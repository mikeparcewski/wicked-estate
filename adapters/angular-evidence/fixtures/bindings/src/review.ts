import { Component, Directive, Input, booleanAttribute, input } from '@angular/core';
import * as ng from '@angular/core';
import { Component as Cmp } from '@angular/core';
import { Child } from './child';

// codex r1 cases: explicit `this` reads, a structural directive input, a keyed two-way binding,
// accessor inputs and getters, and namespace / renamed decorators.
@Directive({ selector: '[myIf]', standalone: true })
export class MyIf {
  @Input() myIf = '';
}

@Component({ selector: 'app-badge', standalone: true, template: `` })
export class Badge {
  @Input() set badge(v: string) {}
}

@Component({
  selector: 'app-review',
  standalone: true,
  imports: [Child, MyIf, Badge],
  template: `
    <app-child [user]="this.current"></app-child>
    <div *myIf="current"></div>
    <app-child [(value)]="nums[idx]"></app-child>
    <app-badge [badge]="computed"></app-badge>
  `,
})
export class Review {
  current = 'c';
  nums = [1];
  idx = 0;
  get computed(): string {
    return 'g';
  }
}

@ng.Component({ selector: 'app-ns', standalone: true, imports: [Child], template: `<app-child [user]="nsField"></app-child>` })
export class NsCmp {
  nsField = 'n';
}

@Cmp({ selector: 'app-alias', standalone: true, imports: [Child], template: `<app-child [user]="aliasField"></app-child>` })
export class AliasCmp {
  aliasField = 'a';
}

// codex r2 cases: setter declared before its getter on the host, a decorator input with a
// transform, and a transformed signal input.
@Component({ selector: 'app-flag', standalone: true, template: `` })
export class Flag {
  @Input({ transform: booleanAttribute }) enabled = false;
  on = input(false, { transform: booleanAttribute });
}

@Component({
  selector: 'app-order',
  standalone: true,
  imports: [Flag, Child],
  template: `<app-flag [enabled]="text" [on]="text"></app-flag><app-child [user]="ordered"></app-child>`,
})
export class Order {
  text = 'true';
  set ordered(v: string) {}
  get ordered(): string {
    return 'o';
  }
}

// codex r3 cases: a host directive exposing an input under a new name (and an unexposed one), a
// component selector on an ng-template, and a quoted `'transform'` key.
@Directive({ selector: '[hd]', standalone: true })
export class Hd {
  @Input() raw = '';
  @Input() hidden = '';
}

@Component({
  selector: 'app-hosted',
  standalone: true,
  template: ``,
  hostDirectives: [{ directive: Hd, inputs: ['raw: renamed'] }],
})
export class Hosted {}

@Component({ selector: '[widget]', standalone: true, template: `` })
export class Widget {
  @Input() value = '';
}

@Component({ selector: 'app-quoted', standalone: true, template: `` })
export class Quoted {
  q = input(false, { 'transform': booleanAttribute });
}

@Component({
  selector: 'app-r3',
  standalone: true,
  imports: [Hosted, Widget, Quoted],
  template: `<app-hosted [renamed]="field" [hidden]="field"></app-hosted><ng-template widget [value]="field"></ng-template><app-quoted [q]="field"></app-quoted>`,
})
export class R3 {
  field = 'f';
}
