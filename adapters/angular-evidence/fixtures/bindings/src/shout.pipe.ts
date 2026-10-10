import { Pipe, PipeTransform } from '@angular/core';

@Pipe({ name: 'shout', standalone: true })
export class ShoutPipe implements PipeTransform {
  transform(v: string): string {
    return v.toUpperCase();
  }
}
