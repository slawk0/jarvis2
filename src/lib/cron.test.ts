import { describe, expect, it } from 'vitest';
import { describeCron, isValidCron } from './cron';

describe('isValidCron', () => {
	it('accepts standard expressions and keywords', () => {
		for (const expr of [
			'* * * * *',
			'*/5 * * * *',
			'0 3 * * 1-5',
			'15,45 8-18/2 1 jan,jul mon',
			'@daily',
			'@reboot',
			'0 0 * * 7'
		]) {
			expect(isValidCron(expr), expr).toBe(true);
		}
	});

	it('rejects malformed expressions', () => {
		for (const expr of [
			'',
			'* * * *',
			'* * * * * *',
			'60 * * * *',
			'* 24 * * *',
			'* * 0 * *',
			'* * * 13 *',
			'*/0 * * * *',
			'a b c d e',
			'@sometimes',
			'1-2-3 * * * *'
		]) {
			expect(isValidCron(expr), expr).toBe(false);
		}
	});
});

describe('describeCron', () => {
	it('describes common schedules', () => {
		expect(describeCron('* * * * *')).toBe('Every minute');
		expect(describeCron('*/5 * * * *')).toBe('Every 5 minutes');
		expect(describeCron('0 * * * *')).toBe('Every hour at minute 0');
		expect(describeCron('30 3 * * *')).toBe('At 03:30 every day');
		expect(describeCron('0 9 * * 1-5')).toBe('At 09:00 on Monday to Friday');
		expect(describeCron('0 0 1 * *')).toBe('At 00:00 on day 1 of the month');
		expect(describeCron('15 2 * 6 0')).toBe('At 02:15 on Sunday in June');
		expect(describeCron('0 */6 * * *')).toBe('Every 6 hours at minute 0');
		expect(describeCron('0 8,20 * * *')).toBe('At 08:00, 20:00 every day');
		expect(describeCron('@weekly')).toBe('Every Sunday at 00:00');
	});

	it('returns an empty string for invalid input', () => {
		expect(describeCron('nonsense')).toBe('');
		expect(describeCron('99 * * * *')).toBe('');
	});
});
