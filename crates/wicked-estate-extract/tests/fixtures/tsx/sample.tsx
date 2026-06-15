import React, { useState, useCallback } from 'react';

export interface CardProps {
    title: string;
    body: string;
    onDismiss: () => void;
}

export enum Variant {
    Info,
    Warning,
    Error,
}

export type VariantKey = keyof typeof Variant;

export const DEFAULT_VARIANT: Variant = Variant.Info;

export const formatTitle = (title: string): string => title.toUpperCase();

export function Badge({ label }: { label: string }): JSX.Element {
    return <span className="badge">{label}</span>;
}

export function Card({ title, body, onDismiss }: CardProps): JSX.Element {
    const [visible, setVisible] = useState(true);
    const handleDismiss = useCallback(() => {
        setVisible(false);
        onDismiss();
    }, [onDismiss]);

    if (!visible) return <></>;
    return (
        <div className="card">
            <h2>{formatTitle(title)}</h2>
            <p>{body}</p>
            <Badge label="new" />
            <button onClick={handleDismiss}>Dismiss</button>
        </div>
    );
}

export function Dashboard(): JSX.Element {
    const [count, setCount] = useState(0);
    return (
        <div>
            <Card
                title="Alert"
                body="Something happened"
                onDismiss={() => setCount(c => c + 1)}
            />
            <p>Dismissed: {count}</p>
        </div>
    );
}
