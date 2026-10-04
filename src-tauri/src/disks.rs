//! Filesystem usage, block devices and partition operations.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use crate::deps::{self, Tool};
use crate::error::{AppError, AppResult};
use crate::jobs::JobMeta;
use crate::shell::{q, validate};
use crate::ssh::session::Exec;
use crate::state::AppState;
use crate::stats::{is_pseudo_fs, parse_partitions};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FsUsage {
    pub device: String,
    pub fs_type: String,
    pub mount: String,
    pub total: u64,
    pub used: u64,
    pub available: u64,
    #[specta(type = i32)]
    pub use_percent: f64,
    /// tmpfs, overlay and other virtual filesystems.
    pub system: bool,
    pub is_loop: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BlockDevice {
    pub name: String,
    pub path: String,
    /// Name of the parent device, `None` for whole disks.
    pub parent: Option<String>,
    pub depth: u32,
    /// `disk`, `part`, `lvm`, `loop`, `rom`, `crypt`, `raid1`…
    pub kind: String,
    pub size: u64,
    pub model: String,
    pub fs_type: String,
    pub mount: String,
    pub label: String,
    pub uuid: String,
    pub partition_table: String,
    pub read_only: bool,
}

#[derive(Deserialize)]
struct LsblkOutput {
    blockdevices: Vec<LsblkNode>,
}

#[derive(Deserialize)]
struct LsblkNode {
    name: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    size: serde_json::Value,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    fstype: Option<String>,
    #[serde(default)]
    mountpoint: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    pttype: Option<String>,
    #[serde(default)]
    ro: serde_json::Value,
    #[serde(default)]
    children: Vec<LsblkNode>,
}

/// Flatten `lsblk -J -b` output into rows with parent links.
pub fn parse_lsblk(json: &str) -> AppResult<Vec<BlockDevice>> {
    let parsed: LsblkOutput = serde_json::from_str(json)?;
    let mut out = Vec::new();
    fn walk(node: LsblkNode, parent: Option<&str>, depth: u32, out: &mut Vec<BlockDevice>) {
        // Older lsblk prints numbers and booleans as strings.
        let size = match &node.size {
            serde_json::Value::Number(n) => n.as_u64().unwrap_or(0),
            serde_json::Value::String(s) => s.parse().unwrap_or(0),
            _ => 0,
        };
        let read_only = match &node.ro {
            serde_json::Value::Bool(b) => *b,
            serde_json::Value::String(s) => s == "1",
            serde_json::Value::Number(n) => n.as_u64() == Some(1),
            _ => false,
        };
        let name = node.name.clone();
        out.push(BlockDevice {
            path: node.path.unwrap_or_else(|| format!("/dev/{}", node.name)),
            name: node.name,
            parent: parent.map(str::to_string),
            depth,
            kind: node.kind.unwrap_or_default(),
            size,
            model: node.model.unwrap_or_default().trim().to_string(),
            fs_type: node.fstype.unwrap_or_default(),
            mount: node.mountpoint.unwrap_or_default(),
            label: node.label.unwrap_or_default(),
            uuid: node.uuid.unwrap_or_default(),
            partition_table: node.pttype.unwrap_or_default(),
            read_only,
        });
        for child in node.children {
            walk(child, Some(&name), depth + 1, out);
        }
    }
    for node in parsed.blockdevices {
        walk(node, None, 0, &mut out);
    }
    Ok(out)
}

/// Only plain device nodes are accepted where a device is an argument.
pub fn device(path: &str) -> AppResult<&str> {
    let ok = path.starts_with("/dev/")
        && path.len() <= 128
        && !path.contains("..")
        && path[5..].bytes().all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b))
        && path.len() > 5;
    if ok {
        Ok(path)
    } else {
        Err(AppError::invalid(format!("Invalid device: {path}")))
    }
}

/// Largest free region from `parted -m -s <disk> unit MiB print free`, as (start, end) in MiB.
pub fn largest_free_region(parted: &str) -> Option<(u64, u64)> {
    parted
        .lines()
        .filter_map(|line| {
            let line = line.trim_end_matches(';');
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() < 5 || fields[4] != "free" {
                return None;
            }
            let mib = |s: &str| s.trim_end_matches("MiB").parse::<f64>().ok();
            let (start, end) = (mib(fields[1])?, mib(fields[2])?);
            // Keep clear of the partition table at the start of the disk.
            let start = start.ceil().max(1.0) as u64;
            let end = end.floor() as u64;
            (end > start + 8).then_some((start, end))
        })
        .max_by_key(|(start, end)| end - start)
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum NewFs {
    Ext4,
    Xfs,
    Fat32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TableKind {
    Gpt,
    Mbr,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PartitionRequest {
    pub disk: String,
    /// Write a new partition table first (destroys everything on the disk).
    pub new_table: Option<TableKind>,
    pub filesystem: NewFs,
    pub label: String,
}

// ---------------------------------------------------------------- commands

#[tauri::command]
#[specta::specta]
pub async fn disks_usage(state: State<'_, AppState>) -> AppResult<Vec<FsUsage>> {
    let session = state.session()?;
    let out = session.run("df -PTk 2>/dev/null; true").await?;
    Ok(parse_partitions(&out, "")
        .into_iter()
        .map(|p| FsUsage {
            system: is_pseudo_fs(&p.fs_type, "") && p.mount != "/",
            is_loop: p.device.starts_with("/dev/loop"),
            device: p.device,
            fs_type: p.fs_type,
            mount: p.mount,
            total: p.total,
            used: p.used,
            available: p.available,
            use_percent: p.use_percent,
        })
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn disks_devices(state: State<'_, AppState>) -> AppResult<Vec<BlockDevice>> {
    let session = state.session()?;
    deps::require(&session, Tool::Lsblk).await?;
    let out = session.run_auto("lsblk -J -b -o NAME,PATH,MODEL,SIZE,TYPE,FSTYPE,MOUNTPOINT,LABEL,UUID,PTTYPE,RO").await?;
    parse_lsblk(&out)
}

#[tauri::command]
#[specta::specta]
pub async fn disk_mount(state: State<'_, AppState>, device_path: String, mount_point: String, create_dir: bool) -> AppResult<()> {
    let session = state.session()?;
    let dev = q(device(&device_path)?);
    let target = q(validate::abs_path(&mount_point)?);
    let mkdir = if create_dir { format!("mkdir -p -- {target} && ") } else { String::new() };
    session.exec(Exec::new(format!("{mkdir}mount -- {dev} {target}")).sudo().secs(60)).await?.into_stdout()?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn disk_unmount(state: State<'_, AppState>, target: String) -> AppResult<()> {
    let session = state.session()?;
    // Either a device or a mount point.
    let target = if target.starts_with("/dev/") { device(&target)? } else { validate::abs_path(&target)? };
    if target == "/" {
        return Err(AppError::invalid("The root filesystem cannot be unmounted"));
    }
    session.exec(Exec::new(format!("umount -- {}", q(target))).sudo().secs(60)).await?.into_stdout()?;
    Ok(())
}

/// Create a partition (optionally a new table first) and a filesystem on it. Streamed job.
#[tauri::command]
#[specta::specta]
pub async fn disk_create_partition(app: AppHandle, state: State<'_, AppState>, request: PartitionRequest) -> AppResult<String> {
    let session = state.session()?;
    let disk = device(&request.disk)?.to_string();
    let label = request.label.trim().to_string();
    if !label.is_empty() {
        validate::slug("label", &label)?;
    }
    deps::require(&session, Tool::Parted).await?;
    session.sudo_plan().await?;
    let d = q(&disk);

    let (range, table_cmd) = match request.new_table {
        Some(kind) => {
            let name = if kind == TableKind::Gpt { "gpt" } else { "msdos" };
            ("1MiB 100%".to_string(), format!("parted -s {d} mklabel {name} && "))
        }
        None => {
            let free = session.run_sudo(format!("parted -m -s {d} unit MiB print free")).await?;
            let (start, end) = largest_free_region(&free).ok_or_else(|| AppError::invalid("There is no unallocated space on this disk"))?;
            (format!("{start}MiB {end}MiB"), String::new())
        }
    };
    let (parted_fs, mkfs) = match request.filesystem {
        NewFs::Ext4 => ("ext4", if label.is_empty() { "mkfs.ext4 -F".to_string() } else { format!("mkfs.ext4 -F -L {}", q(&label)) }),
        NewFs::Xfs => ("xfs", if label.is_empty() { "mkfs.xfs -f".to_string() } else { format!("mkfs.xfs -f -L {}", q(&label)) }),
        NewFs::Fat32 => {
            ("fat32", if label.is_empty() { "mkfs.vfat -F 32".to_string() } else { format!("mkfs.vfat -F 32 -n {}", q(&label)) })
        }
    };
    // The newest partition of the disk is the one just created.
    let script = format!(
        "set -e; before=$(lsblk -nrpo NAME {d} | sort); {table_cmd}parted -s -a optimal {d} -- mkpart primary {parted_fs} {range}; \
         partprobe {d} 2>/dev/null || true; sleep 1; udevadm settle 2>/dev/null || true; \
         after=$(lsblk -nrpo NAME {d} | sort); part=$(printf '%s\\n%s\\n' \"$before\" \"$after\" | sort | uniq -u | tail -n 1); \
         [ -n \"$part\" ] || {{ echo 'Could not find the new partition' >&2; exit 1; }}; \
         echo \"Created $part\"; {mkfs} \"$part\"; echo 'Done.'"
    );
    let meta = JobMeta::visible(format!("Create partition on {disk}"), script.clone());
    state.jobs.start(&app, session, meta, Exec::new(script).sudo()).await
}

/// Grow a partition to fill its disk and then grow the filesystem. Streamed job.
#[tauri::command]
#[specta::specta]
pub async fn disk_expand(app: AppHandle, state: State<'_, AppState>, partition: String) -> AppResult<String> {
    let session = state.session()?;
    let part = device(&partition)?.to_string();
    deps::require(&session, Tool::Growpart).await?;
    let p = q(&part);
    let script = format!(
        "set -e; name=$(basename {p}); disk=/dev/$(lsblk -no PKNAME {p} | head -n 1); num=$(cat /sys/class/block/$name/partition); \
         [ -b \"$disk\" ] && [ -n \"$num\" ] || {{ echo 'Not a partition of a disk' >&2; exit 1; }}; \
         growpart \"$disk\" \"$num\" || [ $? -eq 1 ]; \
         fs=$(lsblk -no FSTYPE {p} | head -n 1); mnt=$(lsblk -no MOUNTPOINT {p} | head -n 1); \
         case \"$fs\" in \
           ext2|ext3|ext4) resize2fs {p} ;; \
           xfs) [ -n \"$mnt\" ] || {{ echo 'An XFS filesystem must be mounted to grow it' >&2; exit 1; }}; xfs_growfs \"$mnt\" ;; \
           btrfs) [ -n \"$mnt\" ] || {{ echo 'A btrfs filesystem must be mounted to grow it' >&2; exit 1; }}; btrfs filesystem resize max \"$mnt\" ;; \
           *) echo \"Partition grown; filesystem '$fs' was not resized automatically\" ;; \
         esac; echo 'Done.'"
    );
    let meta = JobMeta::visible(format!("Expand {part}"), script.clone());
    state.jobs.start(&app, session, meta, Exec::new(script).sudo()).await
}

/// Check a filesystem. A mounted filesystem is only inspected, never repaired.
#[tauri::command]
#[specta::specta]
pub async fn disk_fsck(app: AppHandle, state: State<'_, AppState>, device_path: String) -> AppResult<String> {
    let session = state.session()?;
    let dev = device(&device_path)?.to_string();
    let d = q(&dev);
    let script = format!(
        "if findmnt -rn -S {d} >/dev/null 2>&1; then echo 'Mounted: read-only check (no repairs).'; fsck -n {d}; \
         else fsck -f -y {d}; fi; rc=$?; echo \"fsck exit code $rc\"; [ $rc -lt 4 ]"
    );
    let meta = JobMeta::visible(format!("Check filesystem {dev}"), script.clone());
    state.jobs.start(&app, session, meta, Exec::new(script).sudo()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const LSBLK: &str = r#"{
   "blockdevices": [
      {"name":"loop0","path":"/dev/loop0","model":null,"size":66584576,"type":"loop","fstype":"squashfs","mountpoint":"/snap/core/1","label":null,"uuid":null,"pttype":null,"ro":true},
      {"name":"sda","path":"/dev/sda","model":"QEMU HARDDISK   ","size":107374182400,"type":"disk","fstype":null,"mountpoint":null,"label":null,"uuid":null,"pttype":"gpt","ro":false,
         "children": [
            {"name":"sda1","path":"/dev/sda1","model":null,"size":1127219200,"type":"part","fstype":"vfat","mountpoint":"/boot/efi","label":null,"uuid":"AB12-CD34","pttype":"gpt","ro":false},
            {"name":"sda2","path":"/dev/sda2","model":null,"size":53687091200,"type":"part","fstype":"LVM2_member","mountpoint":null,"label":null,"uuid":"x","pttype":"gpt","ro":false,
               "children": [
                  {"name":"vg-root","path":"/dev/mapper/vg-root","model":null,"size":53682896896,"type":"lvm","fstype":"ext4","mountpoint":"/","label":"root","uuid":"y","pttype":null,"ro":false}
               ]
            }
         ]
      }
   ]
}"#;

    #[test]
    fn flattens_lsblk_tree() {
        let d = parse_lsblk(LSBLK).unwrap();
        let names: Vec<(&str, u32, Option<&str>)> = d.iter().map(|b| (b.name.as_str(), b.depth, b.parent.as_deref())).collect();
        assert_eq!(
            names,
            vec![("loop0", 0, None), ("sda", 0, None), ("sda1", 1, Some("sda")), ("sda2", 1, Some("sda")), ("vg-root", 2, Some("sda2"))]
        );
        assert_eq!(d[1].model, "QEMU HARDDISK");
        assert_eq!(d[1].partition_table, "gpt");
        assert_eq!(d[2].uuid, "AB12-CD34");
        assert_eq!(d[4].path, "/dev/mapper/vg-root");
        assert_eq!(d[4].mount, "/");
        assert!(d[0].read_only);
    }

    #[test]
    fn lsblk_with_string_numbers() {
        let old = r#"{"blockdevices":[{"name":"vda","size":"10737418240","type":"disk","ro":"0","children":[{"name":"vda1","size":"10736369664","type":"part","ro":"1"}]}]}"#;
        let d = parse_lsblk(old).unwrap();
        assert_eq!(d[0].size, 10_737_418_240);
        assert_eq!(d[0].path, "/dev/vda");
        assert!(!d[0].read_only);
        assert!(d[1].read_only);
    }

    #[test]
    fn device_validation() {
        assert!(device("/dev/sda1").is_ok());
        assert!(device("/dev/mapper/vg-root").is_ok());
        assert!(device("/dev/nvme0n1p2").is_ok());
        assert!(device("/dev/").is_err());
        assert!(device("/etc/passwd").is_err());
        assert!(device("/dev/../etc/passwd").is_err());
        assert!(device("/dev/sda; rm -rf /").is_err());
    }

    #[test]
    fn finds_largest_free_region() {
        let out = "BYT;\n/dev/sdb:20480MiB:scsi:512:512:gpt:QEMU:;\n1:0.02MiB:1.00MiB:0.98MiB:free;\n1:1.00MiB:5000MiB:4999MiB:ext4:primary:;\n1:5000MiB:20480MiB:15480MiB:free;\n";
        assert_eq!(largest_free_region(out), Some((5000, 20480)));
        let full = "BYT;\n/dev/sdb:100MiB:scsi:512:512:gpt:QEMU:;\n1:0.02MiB:1.00MiB:0.98MiB:free;\n1:1.00MiB:100MiB:99MiB:ext4::;\n";
        assert_eq!(largest_free_region(full), None);
    }
}
