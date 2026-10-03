import { describe, expect, it } from 'vitest';
import type { Messages } from './i18n';
import { activeStepLabel, buildTranscript, type ActivityPart, type Reply } from './transcript';

// How the transcript is built is core's to test (packages/core/src/transcript.test.ts); this is
// what the web says about it.
describe('activeStepLabel', () => {
	const m = { steps: { summarizing: 'Summarizing' } } as unknown as Messages;

	it('says so while the model is writing a summary', () => {
		const entries = buildTranscript(
			[
				{
					id: 1,
					kind: 'human',
					senderId: 'anna',
					senderName: 'Anna',
					text: 'Files?',
					attachments: [],
					queued: false,
					createdAt: 1
				}
			],
			[{ type: 'compaction', text: '' }],
			true
		);
		const part = (entries[1] as Reply).parts[0] as ActivityPart;
		expect(activeStepLabel(part, {}, false, m)).toBe('Summarizing');
	});
});
