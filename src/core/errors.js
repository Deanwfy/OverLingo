export function readableRuntimeError(error, text) {
    const message = error?.message || String(error || text('unknownError'));
    if (/has not played any audio/i.test(message)) {
        return text('captureApplicationSilent');
    }
    if (/selected application|application-specific audio capture/i.test(message)) {
        return text('captureApplicationUnavailable');
    }
    if (/microphone|input device|input stream|no input/i.test(message)) {
        return text('microphoneUnavailable');
    }
    // Only tap creation is gated on the recording permission; the later Core Audio steps
    // fail for their own reasons and keep their own message rather than misdirecting.
    if (/create audio tap/i.test(message)) {
        return text('audioPermissionRequired');
    }
    if (/timed out|websocket connect|dns|tls|network|connection refused/i.test(message)) {
        return text('networkUnavailable');
    }
    return message;
}
