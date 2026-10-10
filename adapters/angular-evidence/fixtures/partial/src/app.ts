import { Component, Input } from '@angular/core';

@Component({ selector: 'app-leaf', standalone: true, template: `` })
export class Leaf {
  @Input() user = '';
}

// A partial build: one unknown element and one type error. The resolvable binding is still
// emitted; the broken ones are not invented.
@Component({
  selector: 'app-root',
  standalone: true,
  imports: [Leaf],
  template: `<app-leaf [user]="name"></app-leaf><missing-el [x]="name"></missing-el><app-leaf [user]="nope"></app-leaf>`,
})
export class Root {
  name = 'n';
}
