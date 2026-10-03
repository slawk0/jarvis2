import type { AppError, ErrorCode } from './bindings';

/**
 * Human message for every backend error code. The `Record<ErrorCode, …>`
 * type makes a missing code a compile error, so no code can go unmapped.
 */
const MESSAGES: Record<ErrorCode, string> = {
	NOT_CONNECTED: 'Not connected to a server.',
	CONNECTION_FAILED: 'Could not connect to the server.',
	CONNECTION_LOST: 'The connection to the server was lost.',
	AUTH_FAILED: 'Authentication failed. Check the user name and password or key.',
	HOST_KEY_UNKNOWN: 'The server’s host key is not trusted yet.',
	HOST_KEY_CHANGED: 'The server’s host key has changed.',
	KEY_FILE_INVALID: 'The private key file could not be used.',
	KEY_PASSPHRASE_REQUIRED: 'This private key is encrypted. Enter its passphrase in the profile.',
	KEY_PASSPHRASE_WRONG: 'The passphrase for the private key is incorrect.',
	TIMEOUT: 'The operation timed out.',
	CANCELLED: 'Cancelled.',
	SUDO_PASSWORD_REQUIRED: 'Root privileges are required.',
	SUDO_PASSWORD_EXPIRED: 'Your sudo session expired. Enter the password again.',
	SUDO_PASSWORD_WRONG: 'Incorrect sudo password.',
	SUDO_LOCKED: 'Too many incorrect passwords. Try again shortly.',
	SUDO_UNAVAILABLE: 'This action needs root, but sudo is not available for this user.',
	PERMISSION_DENIED: 'Permission denied.',
	NOT_FOUND: 'Not found.',
	ALREADY_EXISTS: 'Already exists.',
	INVALID_INPUT: 'Invalid input.',
	COMMAND_FAILED: 'The command failed.',
	PARSE_FAILED: 'The server returned output that could not be understood.',
	DEPENDENCY_MISSING: 'A required tool is not installed on the server.',
	UNSUPPORTED: 'Not supported on this server.',
	IO: 'A local file operation failed.',
	STORE_CORRUPT: 'A local data file was damaged and has been set aside.',
	KEYRING: 'The system keyring could not be accessed.',
	SFTP: 'The file operation failed.',
	TRANSFER_FAILED: 'The transfer failed.',
	FILE_TOO_LARGE: 'The file is too large to open here.',
	BINARY_FILE: 'This looks like a binary file and cannot be edited as text.',
	DATABASE: 'The database reported an error.',
	DB_NOT_CONNECTED: 'Not connected to the database.',
	HTTP: 'The request failed.',
	PANGOLIN_NOT_CONFIGURED: 'Pangolin is not configured yet.',
	PANGOLIN_UNAUTHORIZED: 'Pangolin rejected the API key.',
	CONFIG_TEST_FAILED: 'The configuration test failed; the change was rolled back.',
	INTERNAL: 'Something went wrong inside Jarvis.'
};

/** Codes where the details are an internal payload, not text for the user. */
const DETAILS_HIDDEN: ReadonlySet<ErrorCode> = new Set([
	'SUDO_PASSWORD_WRONG',
	'SUDO_LOCKED',
	'HOST_KEY_UNKNOWN',
	'HOST_KEY_CHANGED'
]);

export class IpcError extends Error {
	readonly code: ErrorCode;
	readonly details: string | null;

	constructor(code: ErrorCode, details: string | null = null) {
		super(describe(code, details));
		this.name = 'IpcError';
		this.code = code;
		this.details = details;
	}

	/** The mapped message without details. */
	get title(): string {
		return MESSAGES[this.code];
	}

	is(...codes: ErrorCode[]): boolean {
		return codes.includes(this.code);
	}
}

function describe(code: ErrorCode, details: string | null): string {
	const base = MESSAGES[code] ?? MESSAGES.INTERNAL;
	if (!details || DETAILS_HIDDEN.has(code)) return base;
	return `${base.replace(/\.$/, '')}: ${details}`;
}

function isAppError(value: unknown): value is AppError {
	return (
		typeof value === 'object' &&
		value !== null &&
		'code' in value &&
		typeof (value as { code: unknown }).code === 'string' &&
		(value as { code: string }).code in MESSAGES
	);
}

/** Normalise anything thrown into an `IpcError`. */
export function toIpcError(error: unknown): IpcError {
	if (error instanceof IpcError) return error;
	if (isAppError(error)) return new IpcError(error.code, error.details ?? null);
	if (error instanceof Error) return new IpcError('INTERNAL', error.message);
	return new IpcError('INTERNAL', typeof error === 'string' ? error : null);
}

export function errorMessage(error: unknown): string {
	return toIpcError(error).message;
}
