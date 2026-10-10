import { Component } from '@angular/core';
import { Child } from './child';
import { Child as OtherChild } from './other/child';
import { ShoutPipe } from './shout.pipe';
import { LibDir } from '../lib/lib';

@Component({
  selector: 'app-parent',
  standalone: true,
  imports: [Child, OtherChild, ShoutPipe, LibDir],
  template: `
    <app-child [user]="current" [aliasIn]="count + 1" [name]="label" [inherited]="'x'" [(value)]="v"></app-child>
    @if (show) { <other-child [user]="other"></other-child> }
    @for (item of items; track item) { <app-child [user]="item"></app-child> }
    <ng-template #tpl let-x><app-child [user]="x"></app-child></ng-template>
    <input #box /><app-child [user]="box.value"></app-child>
    <app-child [user]="label | shout"></app-child>
    <div [title]="label" libDir [libIn]="label"></div>
    <span>ünï 🙂</span><other-child [user]="current"></other-child>
    <app-child
        [user]="current"></app-child>
  `,
})
export class Parent {
  current = 'a';
  other = 'b';
  count = 1;
  label = 'l';
  v = 2;
  show = true;
  items = ['a'];
}
