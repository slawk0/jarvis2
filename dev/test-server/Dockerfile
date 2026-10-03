# Disposable server for exercising Jarvis end to end (see dev/test-server/README.md).
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        systemd systemd-sysv dbus openssh-server sudo cron ca-certificates curl \
        iproute2 iputils-ping procps util-linux less nano locales \
        nginx ufw iptables restic rclone docker.io docker-compose-v2 \
        default-mysql-client postgresql-client parted \
    && rm -rf /var/lib/apt/lists/* \
    && locale-gen pl_PL.UTF-8 en_US.UTF-8 \
    && systemctl enable ssh cron nginx \
    && systemctl mask systemd-resolved getty@tty1 console-getty docker.service docker.socket containerd.service

# admin: password login, sudo with password.  keyuser: key-only login, passwordless sudo.
ARG ADMIN_PASSWORD=jarvis-test
RUN useradd -m -s /bin/bash -G sudo admin && echo "admin:${ADMIN_PASSWORD}" | chpasswd \
    && useradd -m -s /bin/bash keyuser && passwd -l keyuser \
    && echo 'keyuser ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/keyuser && chmod 440 /etc/sudoers.d/keyuser \
    && mkdir -p /home/keyuser/.ssh && chmod 700 /home/keyuser/.ssh && chown keyuser:keyuser /home/keyuser/.ssh \
    # A non-English default locale proves that parsers do not depend on localized output.
    && echo 'LANG=pl_PL.UTF-8' > /etc/default/locale

# Bind-mounted files keep host ownership, which sshd's StrictModes rejects; copy them in at boot.
COPY jarvis-test-keys.service /etc/systemd/system/jarvis-test-keys.service
RUN systemctl enable jarvis-test-keys.service

EXPOSE 22
STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
