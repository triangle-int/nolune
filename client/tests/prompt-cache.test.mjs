import test from 'node:test';
import assert from 'node:assert/strict';
import {
    cacheChipLabel,
    cacheSegments,
    lastRequestSentence,
    recentRequestLabel,
    sessionSentence,
    systemPromptStatus,
} from '../src/lib/chat/prompt-cache.js';

const reading = (input, read, write) => ({ input_tokens: input, cache_read_tokens: read, cache_write_tokens: write, output_tokens: 40 });
const stats = (last, extra = {}) => ({
    last,
    total: last ?? reading(0, 0, 0),
    requests: last ? 1 : 0,
    hits: last && last.cache_read_tokens > 0 ? 1 : 0,
    recent: last ? [last] : [],
    system_prompt: { turns: 1, changes: 0, last_changed_sections: [] },
    ...extra,
});

test('a hit reads as its share of cached input and a miss says so in words', () => {
    const hit = stats(reading(13_200, 12_300, 800));
    assert.equal(cacheChipLabel(hit), 'cache 93%');
    assert.match(lastRequestSentence(hit), /93% from cache, 12\.3k of 13\.2k input tokens/);

    const miss = stats(reading(9_000, 0, 8_600));
    assert.equal(cacheChipLabel(miss), 'cache miss');
    assert.match(lastRequestSentence(miss), /missed the cache/);
    assert.match(lastRequestSentence(miss), /8\.6k were written for the next request/);
});

test('nothing is claimed before the chat sent a request', () => {
    assert.equal(cacheChipLabel(null), null);
    assert.equal(cacheChipLabel(stats(null)), null);
    assert.match(lastRequestSentence(undefined), /No request/);
    assert.match(sessionSentence(stats(null)), /No request/);
    assert.match(systemPromptStatus(undefined).text, /No turn/);
});

test('the segments split the whole input into read, written and uncached', () => {
    const segments = cacheSegments(reading(1_000, 700, 200));
    assert.deepEqual(segments.map((s) => [s.kind, s.tokens]), [['read', 700], ['write', 200], ['uncached', 100]]);
    assert.equal(Math.round(segments.reduce((sum, s) => sum + s.percent, 0)), 100);
    assert.deepEqual(cacheSegments(reading(0, 0, 0)).map((s) => s.percent), [0, 0, 0]);
});

test('the session line counts hits and the share of all input read from the cache', () => {
    const line = sessionSentence(stats(reading(100, 50, 0), {
        total: reading(20_000, 15_000, 3_000),
        requests: 4,
        hits: 3,
    }));
    assert.match(line, /3 of 4 requests hit the cache/);
    assert.match(line, /75% of all 20\.0k input tokens/);
    assert.match(recentRequestLabel(reading(2_000, 0, 1_900), 0), /^Request 1: cache miss/);
});

test('a system prompt that changed names the section and the cost', () => {
    const steady = systemPromptStatus(stats(null, { system_prompt: { turns: 6, changes: 0, last_changed_sections: [] } }));
    assert.equal(steady.changed, false);
    assert.match(steady.text, /unchanged across 6 turns/);

    const changed = systemPromptStatus(stats(null, { system_prompt: { turns: 6, changes: 2, last_changed_sections: ['skills'] } }));
    assert.equal(changed.changed, true);
    assert.match(changed.text, /changed 2 times in 6 turns, last in skills/);
    assert.match(changed.text, /miss the cache/);
});
