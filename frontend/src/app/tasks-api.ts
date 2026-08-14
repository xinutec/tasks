import { HttpClient } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import {
  Change,
  Holder,
  Me,
  NewTask,
  Revision,
  Session,
  Task,
  TaskDetail,
  Updated,
} from './models';

/** Thin client over the tasks backend. Same-origin in prod; via the dev proxy
 *  (proxy.conf.json) in `ng serve`. The session cookie rides along. */
@Injectable({ providedIn: 'root' })
export class TasksApi {
  private http = inject(HttpClient);

  me(): Observable<Me> {
    return this.http.get<Me>('/api/me');
  }
  logout(): Observable<unknown> {
    return this.http.post('/logout', {});
  }

  list(opts: { done?: boolean } = {}): Observable<Task[]> {
    const params: Record<string, string> = {};
    if (opts.done) params['done'] = 'true';
    return this.http.get<Task[]>('/api/tasks', { params });
  }

  task(id: number): Observable<TaskDetail> {
    return this.http.get<TaskDetail>(`/api/tasks/${id}`);
  }

  create(task: NewTask): Observable<Task> {
    return this.http.post<Task>('/api/tasks', task);
  }

  /** A genuine partial update: send only what is changing. */
  change(id: number, change: Change): Observable<Updated> {
    return this.http.patch<Updated>(`/api/tasks/${id}`, change);
  }

  /**
   * The task as it stood before its most recent edit.
   *
   * 404 when nothing has overwritten it. Only worth calling where
   * `TaskDetail.restorable` is true — it carries a whole previous body, which is
   * exactly what that flag exists to avoid fetching speculatively.
   */
  previous(id: number): Observable<Revision> {
    return this.http.get<Revision>(`/api/tasks/${id}/previous`);
  }

  sessions(): Observable<Session[]> {
    return this.http.get<Session[]>('/api/sessions');
  }

  /** Who holds what: every session, Pippijn, and the pile. */
  holders(): Observable<Holder[]> {
    return this.http.get<Holder[]>('/api/holders');
  }

  /**
   * Give a conversation a name.
   *
   * One column, and it moves nothing: the id is the identity, so every task
   * stays where it is and the lists that resolve a name through the join start
   * saying the new one at once. A session may only rename itself; Pippijn may
   * rename any of them, which is what this call is for.
   */
  rename(id: string, name: string): Observable<void> {
    return this.http.patch<void>(`/api/sessions/${id}`, { name });
  }
}
