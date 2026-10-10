import { Component, Directive, Input, input, model } from '@angular/core';

@Directive()
export abstract class Base {
  @Input() inherited = '';
}

@Component({ selector: 'app-child', standalone: true, template: `<p>{{ user }}</p>` })
export class Child extends Base {
  @Input() user = '';
  @Input('aliasIn') renamed = 0;
  name = input<string>('');
  value = model<number>(0);

  shout(): string {
    const loud = this.user;
    return loud;
  }
}
