import { describe, expect, it } from 'vitest';
import { ipMatches, parseIpList } from './client';

describe('parseIpList', () => {
	it('splits on commas and whitespace', () => {
		expect(parseIpList(' 1.2.3.4, 10.0.*\n2001:db8::1  ')).toEqual(['1.2.3.4', '10.0.*', '2001:db8::1']);
		expect(parseIpList('')).toEqual([]);
	});
});

describe('ipMatches', () => {
	it('matches exact addresses and trailing-star prefixes', () => {
		expect(ipMatches('1.2.3.4', ['1.2.3.4'])).toBe(true);
		expect(ipMatches('1.2.3.40', ['1.2.3.4'])).toBe(false);
		expect(ipMatches('10.0.5.6', ['10.0.*'])).toBe(true);
		expect(ipMatches('10.1.5.6', ['10.0.*'])).toBe(false);
		expect(ipMatches('1.2.3.4', [])).toBe(false);
	});
});
