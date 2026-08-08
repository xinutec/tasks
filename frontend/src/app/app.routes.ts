import { Routes } from '@angular/router';

import { ListView } from './list-view';
import { NewView } from './new-view';
import { TaskView } from './task-view';

/**
 * Routes for the SPA — a real table (fleet convention):
 *
 *   /         → the open list, filtered by repo (?repo=) and holder (?who=)
 *   /t/:id    → one task: its prose, its status, who holds it, its history
 *   /new      → file one
 */
export const routes: Routes = [
  { path: '', component: ListView },
  { path: 't/:id', component: TaskView },
  { path: 'new', component: NewView },
  { path: '**', redirectTo: '' },
];
