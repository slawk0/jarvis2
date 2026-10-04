// Drive the running dev app through the WebView's DevTools protocol.
//
// Start the app with remote debugging enabled (Windows / WebView2):
//   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"; pnpm tauri dev
//
// Usage:
//   node dev/cdp.mjs shot out.png            screenshot of the window
//   node dev/cdp.mjs eval "<js expression>"  evaluate (awaits promises), prints JSON
//   node dev/cdp.mjs click "<css selector>"  click the first matching element
//   node dev/cdp.mjs text "<button text>"    click the first button/link with this text
//   node dev/cdp.mjs type "<css selector>" "<text>"
//   node dev/cdp.mjs key "<Key>" [ctrl] [shift] [alt]
//   node dev/cdp.mjs errors                  console errors captured so far
import { writeFileSync } from 'node:fs';

const PORT = process.env.CDP_PORT ?? 9222;
const [, , command, ...args] = process.argv;

const pages = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
const page = pages.find((p) => p.type === 'page' && !p.url.startsWith('devtools://'));
if (!page) {
	console.error('No page found. Is the app running with remote debugging?');
	process.exit(1);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
	ws.addEventListener('open', resolve, { once: true });
	ws.addEventListener('error', reject, { once: true });
});

let nextId = 0;
const pending = new Map();
ws.addEventListener('message', (event) => {
	const message = JSON.parse(event.data);
	const waiter = pending.get(message.id);
	if (waiter) {
		pending.delete(message.id);
		if (message.error) waiter.reject(new Error(message.error.message));
		else waiter.resolve(message.result);
	}
});

function send(method, params = {}) {
	const id = ++nextId;
	ws.send(JSON.stringify({ id, method, params }));
	return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
}

async function evaluate(expression) {
	const result = await send('Runtime.evaluate', {
		expression,
		awaitPromise: true,
		returnByValue: true,
		userGesture: true
	});
	if (result.exceptionDetails) {
		const detail = result.exceptionDetails;
		throw new Error(detail.exception?.description ?? detail.text);
	}
	return result.result.value;
}

const js = (value) => JSON.stringify(value);

try {
	switch (command) {
		case 'shot': {
			const { data } = await send('Page.captureScreenshot', { format: 'png' });
			writeFileSync(args[0] ?? 'shot.png', Buffer.from(data, 'base64'));
			break;
		}
		case 'eval':
			console.log(JSON.stringify(await evaluate(args[0]), null, 2));
			break;
		case 'click':
			console.log(
				await evaluate(`(() => { const el = document.querySelector(${js(args[0])});
					if (!el) return 'not found'; el.scrollIntoView({block:'center'}); el.click(); return 'clicked'; })()`)
			);
			break;
		case 'text':
			console.log(
				await evaluate(`(() => { const want = ${js(args[0])}.toLowerCase();
					const els = [...document.querySelectorAll('button, a, [role=menuitem], [role=tab], [role=option], [role=radio]')];
					const visible = els.filter((e) => e.offsetParent !== null);
					const el = visible.find((e) => e.textContent.trim().toLowerCase() === want)
						?? visible.find((e) => (e.getAttribute('aria-label') ?? '').toLowerCase() === want)
						?? visible.find((e) => e.textContent.trim().toLowerCase().includes(want));
					if (!el) return 'not found'; el.scrollIntoView({block:'center'}); el.click(); return 'clicked: ' + el.textContent.trim().slice(0, 60); })()`)
			);
			break;
		case 'type':
			console.log(
				await evaluate(`(() => { const el = document.querySelector(${js(args[0])});
					if (!el) return 'not found'; el.focus();
					const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
					Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${js(args[1] ?? '')});
					el.dispatchEvent(new Event('input', { bubbles: true }));
					el.dispatchEvent(new Event('change', { bubbles: true })); return 'typed'; })()`)
			);
			break;
		case 'insert':
			// Types into whatever has focus (works for Monaco and xterm).
			await send('Input.insertText', { text: args[0] });
			console.log('inserted');
			break;
		case 'key': {
			const mods = args.slice(1);
			const modifiers =
				(mods.includes('alt') ? 1 : 0) | (mods.includes('ctrl') ? 2 : 0) | (mods.includes('shift') ? 8 : 0);
			const key = args[0];
			const NAMED = {
				Enter: 13,
				Escape: 27,
				Tab: 9,
				Backspace: 8,
				Delete: 46,
				ArrowLeft: 37,
				ArrowUp: 38,
				ArrowRight: 39,
				ArrowDown: 40,
				F5: 116
			};
			const vk = key.length === 1 ? key.toUpperCase().charCodeAt(0) : NAMED[key];
			const code = key.length === 1 ? (/[a-z]/i.test(key) ? `Key${key.toUpperCase()}` : undefined) : key;
			const base = { key, code, modifiers, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk };
			await send('Input.dispatchKeyEvent', { type: 'keyDown', ...base });
			await send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
			break;
		}
		case 'errors':
			console.log(JSON.stringify(await evaluate('window.__jarvisErrors ?? []'), null, 2));
			break;
		default:
			console.error('unknown command');
			process.exitCode = 1;
	}
} catch (error) {
	console.error(String(error));
	process.exitCode = 1;
} finally {
	ws.close();
}
