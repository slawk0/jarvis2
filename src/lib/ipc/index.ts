/**
 * The typed gateway to the backend.
 *
 * `api.someCommand(args)` resolves with the command's data or throws an
 * `IpcError`. When a command reports that it needs root, the global sudo
 * dialog opens and — after a correct password — the original call is retried
 * automatically, so features never deal with elevation themselves.
 *
 * `apiQuiet` is the same without the automatic prompt, for background polling
 * that must not pop a dialog on its own.
 */
import { commands } from './bindings';
import { IpcError, toIpcError } from './errors';
import { sudo } from '$lib/services/sudo.svelte';

export * from './bindings';
export { IpcError, toIpcError, errorMessage } from './errors';

type Commands = typeof commands;
type AnyFn = (...args: unknown[]) => Promise<unknown>;

/** "composeAction" → "compose action", used as the default elevation reason. */
function humanize(name: string): string {
	return name.replace(/([a-z0-9])([A-Z])/g, '$1 $2').toLowerCase();
}

async function call(name: string, fn: AnyFn, args: unknown[], prompt: boolean): Promise<unknown> {
	for (;;) {
		try {
			return await fn(...args);
		} catch (raw) {
			const error = toIpcError(raw);
			if (!prompt || !error.is('SUDO_PASSWORD_REQUIRED', 'SUDO_PASSWORD_EXPIRED')) throw error;
			const granted = await sudo.request({
				action: humanize(name),
				expired: error.code === 'SUDO_PASSWORD_EXPIRED'
			});
			if (!granted) throw new IpcError('CANCELLED');
		}
	}
}

function wrap(prompt: boolean): Commands {
	return new Proxy(commands, {
		get(target, prop: string) {
			const fn = target[prop as keyof Commands] as unknown;
			if (typeof fn !== 'function') return fn;
			return (...args: unknown[]) => call(prop, fn as AnyFn, args, prompt);
		}
	});
}

export const api: Commands = wrap(true);
export const apiQuiet: Commands = wrap(false);
