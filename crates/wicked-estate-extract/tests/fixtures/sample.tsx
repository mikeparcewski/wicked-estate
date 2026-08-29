import React, { useState } from 'react';

export interface ButtonProps {
    label: string;
    onClick: () => void;
}

export enum Theme {
    Light,
    Dark,
}

export type ThemeMode = Theme;

export const DEFAULT_THEME: Theme = Theme.Light;

export class ThemeProvider {
    current: Theme = Theme.Light;
    // scm-anchors D6: object-valued fields (public + private) are Field defs
    palette = { swatch() {} };
    #cache = { clear() {} };

    toggle(): void {
        this.current = this.current === Theme.Light ? Theme.Dark : Theme.Light;
    }
}

export const handleClick = (e: MouseEvent): void => {
    e.preventDefault();
};

export function Button({ label, onClick }: ButtonProps): JSX.Element {
    const [pressed, setPressed] = useState(false);
    const handle = () => {
        setPressed(true);
        onClick();
    };
    return <button onClick={handle}>{label}</button>;
}

export function App(): JSX.Element {
    const provider = new ThemeProvider();
    provider.toggle();
    return <Button label="click" onClick={() => provider.toggle()} />;
}
