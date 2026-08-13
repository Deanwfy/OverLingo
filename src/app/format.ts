export function formatDuration(seconds: number) {
    const safe = Math.max(0, Number(seconds) || 0);
    const hours = String(Math.floor(safe / 3600)).padStart(2, '0');
    const minutes = String(Math.floor(safe % 3600 / 60)).padStart(2, '0');
    const secs = String(Math.floor(safe % 60)).padStart(2, '0');
    return `${hours}:${minutes}:${secs}`;
}

export function formatDate(value: string) {
    const date = new Date(value);
    return Number.isNaN(date.valueOf())
        ? value
        : new Intl.DateTimeFormat(undefined, {
            month: 'short',
            day: 'numeric',
            hour: '2-digit',
            minute: '2-digit',
        }).format(date);
}

export function formatTime(value: string | number) {
    return new Intl.DateTimeFormat(undefined, {
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
    }).format(new Date(value));
}
