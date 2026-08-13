export function readableRuntimeError(error, text) {
    const message = error?.message || String(error || text('routeFailed'));
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
    if (/api.?key|unauthori[sz]ed|authentication|invalid token|401/i.test(message)) {
        return text('credentialRejected');
    }
    if (/workspace|forbidden|permission|access denied|403/i.test(message)) {
        return text('workspaceRejected');
    }
    if (/rate.?limit|quota|too many|concurren|429/i.test(message)) {
        return text('providerBusy');
    }
    if (/timed out|websocket connect|dns|tls|network|connection refused/i.test(message)) {
        return text('networkUnavailable');
    }
    return message;
}
