import { holderLabel } from './holder';
import { Task } from './models';

/**
 * Whether a task answers what was typed: every word must appear in the subject,
 * the holder or the filer. `#` and digits is only ever an id; bare digits may be either.
 */
export function matches(task: Task, query: string): boolean {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const text = [task.subject, holderLabel(task.assignee), task.filed_by ?? '']
    .join(' ')
    .toLowerCase();
  return words.every((word) => {
    const id = /^#(\d+)$/.exec(word);
    if (id) return task.id === Number(id[1]);
    return (/^\d+$/.test(word) && task.id === Number(word)) || text.includes(word);
  });
}
