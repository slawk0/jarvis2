/**
 * The one confirm / prompt service (no native `confirm()` or `prompt()`).
 *
 *   if (await confirm({ title: 'Delete user?', destructive: true })) …
 *   const name = await prompt({ title: 'Rename', label: 'New name', value });
 */

export interface ConfirmOptions {
	title: string;
	message?: string;
	/** Preformatted details (lists of packages, command output…). */
	detail?: string;
	confirmLabel?: string;
	cancelLabel?: string;
	destructive?: boolean;
	/** The user must type this exact text to enable the confirm button. */
	typeToConfirm?: string;
	/** The user must tick a checkbox with this label ("I understand…"). */
	acknowledge?: string;
}

export interface PromptOptions {
	title: string;
	message?: string;
	label?: string;
	value?: string;
	placeholder?: string;
	confirmLabel?: string;
	password?: boolean;
	multiline?: boolean;
	/** Return an error message to block submission, or null when valid. */
	validate?: (value: string) => string | null;
}

type Request =
	| { kind: 'confirm'; options: ConfirmOptions; resolve: (ok: boolean) => void }
	| { kind: 'prompt'; options: PromptOptions; resolve: (value: string | null) => void };

class DialogService {
	/** The request currently shown; later ones wait their turn. */
	current = $state<Request | null>(null);
	#queue: Request[] = [];

	#enqueue(request: Request): void {
		if (this.current) this.#queue.push(request);
		else this.current = request;
	}

	confirm(options: ConfirmOptions): Promise<boolean> {
		return new Promise((resolve) => this.#enqueue({ kind: 'confirm', options, resolve }));
	}

	prompt(options: PromptOptions): Promise<string | null> {
		return new Promise((resolve) => this.#enqueue({ kind: 'prompt', options, resolve }));
	}

	/** Settle the visible request and show the next one. */
	settle(result: boolean | string | null): void {
		const request = this.current;
		if (!request) return;
		if (request.kind === 'confirm') request.resolve(result === true);
		else request.resolve(typeof result === 'string' ? result : null);
		this.current = this.#queue.shift() ?? null;
	}
}

export const dialogs = new DialogService();

export const confirm = (options: ConfirmOptions) => dialogs.confirm(options);
export const prompt = (options: PromptOptions) => dialogs.prompt(options);
