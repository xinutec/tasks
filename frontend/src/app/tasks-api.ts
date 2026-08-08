import { HttpClient } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { Change, Holder, Me, NewTask, RepoCount, Session, Task, TaskDetail } from './models';

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

  /** `repos` is comma-separated on the wire — see `repo_list` in routes/api.rs. */
  list(opts: { repos?: string[]; done?: boolean } = {}): Observable<Task[]> {
    const params: Record<string, string> = {};
    if (opts.repos?.length) params['repos'] = opts.repos.join(',');
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
  change(id: number, change: Change): Observable<Task> {
    return this.http.patch<Task>(`/api/tasks/${id}`, change);
  }

  sessions(): Observable<Session[]> {
    return this.http.get<Session[]>('/api/sessions');
  }

  /** Who holds what: every session, Pippijn, and the pile. */
  holders(): Observable<Holder[]> {
    return this.http.get<Holder[]>('/api/holders');
  }

  repos(): Observable<RepoCount[]> {
    return this.http.get<RepoCount[]>('/api/repos');
  }
}
