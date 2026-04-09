import { describe, it, expect } from 'vitest';
import { optimize, version } from '../index.js';

describe('optimize', () => {
  it('optimizes SVG with defaults', () => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg"><g><rect width="10" height="10"/></g></svg>';
    const result = optimize(svg);
    expect(result.data).toBeTruthy();
    expect(result.iterations).toBeGreaterThanOrEqual(1);
  });

  it('accepts safe preset', () => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10"/></svg>';
    const result = optimize(svg, { preset: 'safe' });
    expect(result.data).toBeTruthy();
  });

  it('accepts pass overrides', () => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg"><desc>Created with Figma</desc><rect width="10" height="10"/></svg>';
    const result = optimize(svg, { passes: { removeDesc: true } });
    expect(result.data).not.toContain('<desc>');
  });

  it('throws on invalid SVG', () => {
    expect(() => optimize('<not valid xml')).toThrow();
  });

  it('throws on unknown preset', () => {
    expect(() => optimize('<svg/>', { preset: 'unknown' })).toThrow('unknown preset');
  });

  it('throws on unknown pass', () => {
    expect(() => optimize('<svg/>', { passes: { nonExistent: true } })).toThrow('unknown pass');
  });
});

describe('version', () => {
  it('returns version string', () => {
    const v = version();
    expect(v).toMatch(/^\d+\.\d+\.\d+/);
  });
});
