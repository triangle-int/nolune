import type { SubmitFunction } from '@sveltejs/kit';

/** What saving or removing a custom provider says, for the row it names (`''`: the add form). */
export interface CustomProviderResult {
	customProvider?: string;
	customMessage?: string;
	customWarning?: string;
	customError?: string;
}

/**
 * A custom provider form's submit: busy while the server is asked, the fields kept when it
 * fails, and `done` after a save.
 */
export function submitting(busy: (value: boolean) => void, done: () => void): SubmitFunction {
	return () => {
		busy(true);
		return async ({ result, update }) => {
			await update({ reset: false });
			busy(false);
			if (result.type === 'success') done();
		};
	};
}
