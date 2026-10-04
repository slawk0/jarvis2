/** Validation and plain-English descriptions of cron expressions. */

const KEYWORDS: Record<string, string> = {
	'@reboot': 'At every boot',
	'@yearly': 'Once a year (1 January, 00:00)',
	'@annually': 'Once a year (1 January, 00:00)',
	'@monthly': 'On the first day of every month at 00:00',
	'@weekly': 'Every Sunday at 00:00',
	'@daily': 'Every day at 00:00',
	'@midnight': 'Every day at 00:00',
	'@hourly': 'Every hour'
};

const MONTHS = [
	'January',
	'February',
	'March',
	'April',
	'May',
	'June',
	'July',
	'August',
	'September',
	'October',
	'November',
	'December'
];
const DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
const NAMES = /^(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec|sun|mon|tue|wed|thu|fri|sat)$/i;

const RANGES: [number, number][] = [
	[0, 59],
	[0, 23],
	[1, 31],
	[1, 12],
	[0, 7]
];

function valueOk(value: string, [min, max]: [number, number]): boolean {
	if (NAMES.test(value)) return true;
	if (!/^\d+$/.test(value)) return false;
	const n = Number(value);
	return n >= min && n <= max;
}

function fieldOk(field: string, range: [number, number]): boolean {
	return field.split(',').every((part) => {
		const [base, step, extra] = part.split('/');
		if (extra !== undefined || (step !== undefined && !/^[1-9]\d*$/.test(step))) return false;
		if (base === '*') return true;
		const bounds = base.split('-');
		return bounds.length <= 2 && bounds.every((b) => valueOk(b, range));
	});
}

export function isValidCron(expression: string): boolean {
	const expr = expression.trim();
	if (expr in KEYWORDS) return true;
	const fields = expr.split(/\s+/);
	return fields.length === 5 && fields.every((f, i) => fieldOk(f, RANGES[i]));
}

const pad = (n: string) => n.padStart(2, '0');
const isNumber = (s: string) => /^\d+$/.test(s);

function dayName(value: string): string {
	if (isNumber(value)) return DAYS[Number(value) % 7];
	return DAYS.find((d) => d.toLowerCase().startsWith(value.toLowerCase())) ?? value;
}

function monthName(value: string): string {
	if (isNumber(value)) return MONTHS[Number(value) - 1] ?? value;
	return MONTHS.find((m) => m.toLowerCase().startsWith(value.toLowerCase())) ?? value;
}

function listOf(field: string, name: (v: string) => string): string {
	return field
		.split(',')
		.map((part) => {
			const [a, b] = part.split('-');
			return b ? `${name(a)} to ${name(b)}` : name(a);
		})
		.join(', ');
}

/** A short human description, or an empty string for an invalid expression. */
export function describeCron(expression: string): string {
	const expr = expression.trim();
	if (expr in KEYWORDS) return KEYWORDS[expr];
	if (!isValidCron(expr)) return '';
	const [minute, hour, dom, month, dow] = expr.split(/\s+/);

	let time: string;
	const everyMinute = /^\*\/(\d+)$/.exec(minute);
	const everyHour = /^\*\/(\d+)$/.exec(hour);
	if (minute === '*' && hour === '*') time = 'Every minute';
	else if (everyMinute && hour === '*') time = `Every ${everyMinute[1]} minutes`;
	else if (isNumber(minute) && hour === '*') time = `Every hour at minute ${Number(minute)}`;
	else if (isNumber(minute) && everyHour) time = `Every ${everyHour[1]} hours at minute ${Number(minute)}`;
	else if (isNumber(minute) && isNumber(hour)) time = `At ${pad(hour)}:${pad(minute)}`;
	else if (isNumber(minute) && /^[\d,]+$/.test(hour))
		time = `At ${hour
			.split(',')
			.map((h) => `${pad(h)}:${pad(minute)}`)
			.join(', ')}`;
	else if (everyMinute) time = `Every ${everyMinute[1]} minutes during hour ${hour}`;
	else time = `At minute ${minute} past hour ${hour}`;

	const parts = [time];
	if (dow !== '*') parts.push(`on ${listOf(dow, dayName)}`);
	if (dom !== '*')
		parts.push(/^\*\/(\d+)$/.test(dom) ? `every ${dom.slice(2)} days` : `on day ${dom} of the month`);
	if (month !== '*') parts.push(`in ${listOf(month, monthName)}`);
	if (dow === '*' && dom === '*' && month === '*' && !time.startsWith('Every')) parts.push('every day');
	return parts.join(' ');
}

export interface CronPreset {
	value: string;
	label: string;
}

/** Preset choices for each of the five fields. */
export const CRON_FIELDS: { name: string; presets: CronPreset[] }[] = [
	{
		name: 'Minute',
		presets: [
			{ value: '*', label: 'Every minute' },
			{ value: '*/5', label: 'Every 5 minutes' },
			{ value: '*/15', label: 'Every 15 minutes' },
			{ value: '*/30', label: 'Every 30 minutes' },
			{ value: '0', label: 'At minute 0' },
			{ value: '30', label: 'At minute 30' }
		]
	},
	{
		name: 'Hour',
		presets: [
			{ value: '*', label: 'Every hour' },
			{ value: '*/2', label: 'Every 2 hours' },
			{ value: '*/6', label: 'Every 6 hours' },
			{ value: '0', label: 'At midnight' },
			{ value: '3', label: 'At 03:00' },
			{ value: '12', label: 'At noon' }
		]
	},
	{
		name: 'Day of month',
		presets: [
			{ value: '*', label: 'Every day' },
			{ value: '1', label: 'On the 1st' },
			{ value: '15', label: 'On the 15th' },
			{ value: '*/2', label: 'Every 2 days' }
		]
	},
	{
		name: 'Month',
		presets: [
			{ value: '*', label: 'Every month' },
			{ value: '1', label: 'January' },
			{ value: '*/3', label: 'Every 3 months' },
			{ value: '*/6', label: 'Every 6 months' }
		]
	},
	{
		name: 'Weekday',
		presets: [
			{ value: '*', label: 'Any weekday' },
			{ value: '1-5', label: 'Monday to Friday' },
			{ value: '6,0', label: 'Weekends' },
			{ value: '1', label: 'Monday' },
			{ value: '0', label: 'Sunday' }
		]
	}
];
