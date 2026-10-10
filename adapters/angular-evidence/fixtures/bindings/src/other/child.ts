import { Component, Input } from '@angular/core';

// Same class name as src/child.ts: each binding must resolve to its own file.
@Component({ selector: 'other-child', standalone: true, template: `` })
export class Child {
  @Input() user = '';
}
