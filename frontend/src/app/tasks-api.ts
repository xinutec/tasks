import { HttpClient } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import {
  Change,
  Created,
  Holder,
  Me,
  NewTask,
  Revision,
  Session,
  Task,
  TaskDetail,
  Updated,
} from './models';

/** Thin client over the backend: same-origin, or the dev proxy in `ng serve`. */
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

  create(task: NewTask): Observable<Created> {
    return this.http.post<Created>('/api/tasks', task);
  }

  /** A genuine partial update: send only what is changing. */
  change(id: number, change: Change): Observable<Updated> {
    return this.http.patch<Updated>(`/api/tasks/${id}`, change);
  }

  /**
   * The version before the last edit (404 if none); call only when `restorable`, as it is a
   * whole body.
   */
  previous(id: number): Observable<Revision> {
    return this.http.get<Revision>(`/api/tasks/${id}/previous`);
  }

  sessions(): Observable<Session[]> {
    return this.http.get<Session[]>('/api/sessions');
  }

  /** Who holds what: sessions, the person, the pile. */
  holders(): Observable<Holder[]> {
    return this.http.get<Holder[]>('/api/holders');
  }

  /** Name a conversation. The id is the identity, so nothing moves. */
  rename(id: string, name: string): Observable<void> {
    return this.http.patch<void>(`/api/sessions/${id}`, { name });
  }
}
