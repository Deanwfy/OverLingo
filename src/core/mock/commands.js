import { formatDuration } from '../../app/format';
import { readJson, writeJson } from './storage.js';

export async function invoke(command, args = {}) {
    switch (command) {
        case 'get_credential_status':
            return readJson('overlingo-credentials') || { qwen: true, openai: false };
        case 'set_provider_credential': {
            const status = readJson('overlingo-credentials') || { qwen: false, openai: false };
            status[args.provider] = Boolean(args.secret?.trim());
            writeJson('overlingo-credentials', status);
            return status;
        }
        case 'list_sessions':
            return sessionList();
        case 'read_session': {
            const session = (readJson('overlingo-sessions') || {})[args.id];
            if (!session) throw new Error('Session not found');
            return session;
        }
        case 'rename_session': {
            const sessions = readJson('overlingo-sessions') || {};
            const session = sessions[args.id];
            if (!session) throw new Error('Session not found');
            session.json.title = args.title;
            session.md = renderMarkdown(session.json, 'both');
            writeJson('overlingo-sessions', sessions);
            return null;
        }
        case 'export_session': {
            const session = (readJson('overlingo-sessions') || {})[args.id];
            if (!session) throw new Error('Session not found');
            downloadFile(
                `${session.json.title}.txt`,
                renderTranscript(session.json, args.mode, args.labels),
            );
            return true;
        }
        case 'delete_session': {
            const sessions = readJson('overlingo-sessions') || {};
            delete sessions[args.id];
            writeJson('overlingo-sessions', sessions);
            return null;
        }
        default:
            throw new Error(`Unsupported browser command: ${command}`);
    }
}

export async function getAutostartEnabled() {
    return localStorage.getItem('overlingo-autostart') === 'true';
}

export async function setAutostartEnabled(enabled) {
    localStorage.setItem('overlingo-autostart', String(enabled));
}

function sessionList() {
    const sessions = Object.values(readJson('overlingo-sessions') || {});
    return sessions.map(({ json }) => ({
        id: json.id,
        title: json.title,
        created_at: json.created_at,
        duration_sec: json.duration_sec,
    })).sort((a, b) => b.created_at.localeCompare(a.created_at));
}

function renderMarkdown(data, mode) {
    const lines = [`# ${data.title}`, '', data.created_at, ''];
    for (const segment of data.chunks.flatMap(chunk => chunk.segments)) {
        const source = mode === 'target' ? '' : segment.src;
        const target = mode === 'source' ? '' : segment.tgt;
        if (!source && !target) continue;
        lines.push(`## ${segment.ts} · ${segment.route_id}`, '');
        if (source) lines.push(`**${segment.source_lang}**  ${source}`);
        if (target) lines.push(`**${segment.target_lang}**  ${target}`);
        lines.push('');
    }
    return lines.join('\n');
}

function renderTranscript(data, mode, labels = {}) {
    const lines = [data.title, `${data.created_at} · ${formatDuration(data.duration_sec)}`, ''];
    for (const segment of data.chunks.flatMap(chunk => chunk.segments)) {
        const source = mode === 'target' ? '' : segment.src;
        const target = mode === 'source' ? '' : segment.tgt;
        if (!source && !target) continue;
        lines.push(`[${segment.ts}] ${labels[segment.route_id] || segment.route_id}`);
        if (source) lines.push(source);
        if (target) lines.push(target);
        lines.push('');
    }
    return lines.join('\n');
}

function downloadFile(name, content) {
    const url = URL.createObjectURL(new Blob([content], { type: 'text/markdown' }));
    const link = document.createElement('a');
    link.href = url;
    link.download = name;
    link.click();
    URL.revokeObjectURL(url);
}
