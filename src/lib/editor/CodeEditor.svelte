<!-- Monaco wrapper: bindable value, language, Ctrl+S / Ctrl+Enter hooks, live syntax checks. -->
<script lang="ts">
	import { onMount } from 'svelte';
	import StateView from '$lib/components/StateView.svelte';
	import { settings } from '$lib/services/settings.svelte';
	import { cn, debounce } from '$lib/utils';

	type Monaco = typeof import('./monaco');

	interface Props {
		value: string;
		language?: string;
		readonly?: boolean;
		/** Number of syntax problems found (JSON / YAML / TOML / XML / INI). */
		problems?: number;
		onsave?: () => void;
		onrun?: () => void;
		minimap?: boolean;
		class?: string;
	}

	let {
		value = $bindable(''),
		language = 'plaintext',
		readonly = false,
		// eslint-disable-next-line no-useless-assignment -- written for the parent through the binding
		problems = $bindable(0),
		onsave,
		onrun,
		minimap = false,
		class: className
	}: Props = $props();

	let host = $state<HTMLDivElement | null>(null);
	let lib = $state<Monaco | null>(null);
	let editor: import('monaco-editor').editor.IStandaloneCodeEditor | null = null;
	let applying = false;

	const validate = debounce(() => {
		const model = editor?.getModel();
		if (!lib || !model) return;
		if (language === 'json') {
			problems = lib.monaco.editor.getModelMarkers({ resource: model.uri }).length;
			return;
		}
		const found = lib.checkSyntax(language, model.getValue());
		lib.applyProblems(model, found);
		problems = found.length;
	}, 300);

	onMount(() => {
		let disposed = false;
		void import('./monaco').then((loaded) => {
			if (disposed || !host) return;
			lib = loaded;
			const { monaco } = loaded;
			editor = monaco.editor.create(host, {
				value,
				language,
				readOnly: readonly,
				theme: settings.isDark ? 'jarvis-dark' : 'jarvis-light',
				automaticLayout: true,
				minimap: { enabled: minimap },
				fontFamily: "'JetBrains Mono Variable', monospace",
				fontSize: 12.5,
				scrollBeyondLastLine: false,
				tabSize: 2,
				renderWhitespace: 'selection',
				contextmenu: true,
				fixedOverflowWidgets: true
			});
			editor.onDidChangeModelContent(() => {
				if (applying) return;
				value = editor!.getValue();
				validate();
			});
			editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => onsave?.());
			editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => onrun?.());
			validate();
		});
		return () => {
			disposed = true;
			validate.cancel();
			editor?.getModel()?.dispose();
			editor?.dispose();
			editor = null;
		};
	});

	// Push outside changes of `value` into the editor.
	$effect(() => {
		const next = value;
		if (editor && editor.getValue() !== next) {
			applying = true;
			editor.setValue(next);
			applying = false;
			validate();
		}
	});

	$effect(() => {
		const model = lib && editor?.getModel();
		if (lib && model) {
			lib.monaco.editor.setModelLanguage(model, language);
			validate();
		}
	});

	$effect(() => {
		editor?.updateOptions({ readOnly: readonly });
	});

	$effect(() => {
		lib?.monaco.editor.setTheme(settings.isDark ? 'jarvis-dark' : 'jarvis-light');
	});

	/** Selected text, or everything when nothing is selected. */
	export function selectionOrAll(): string {
		const selection = editor?.getSelection();
		const model = editor?.getModel();
		if (selection && model && !selection.isEmpty()) return model.getValueInRange(selection);
		return editor?.getValue() ?? value;
	}

	export function focus(): void {
		editor?.focus();
	}
</script>

<div class={cn('relative min-h-0 overflow-hidden', className)} data-native-menu>
	<div bind:this={host} class="absolute inset-0"></div>
	{#if !lib}
		<StateView kind="loading" title="Loading editor…" class="absolute inset-0" />
	{/if}
</div>
