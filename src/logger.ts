export function info(...args: any[]) {
    if (process.env.LOG_LEVEL !== 'NONE' && process.env.LOG_LEVEL !== 'ERROR') {
        console.log(...args);
    }
}
export function error(...args: any[]) {
    if (process.env.LOG_LEVEL !== 'NONE') {
        console.error(...args);
    }
}
export function debug(...args: any[]) {
    if (process.env.LOG_LEVEL === 'DEBUG') {
        console.log(...args);
    }
}
