import type { ControllerAction, UpdateStatus } from '../app/types';

export function invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T>;
export function subscribeController<T>(
    surface: 'main' | 'overlay' | 'toolbar' | 'panel',
    handler: (snapshot: T) => void,
): Promise<() => void>;
export function sendControllerAction(request: ControllerAction): Promise<void>;
export function showSettingsWindow(): Promise<void>;
export function onOverlayOutsideClick(handler: () => void): Promise<() => void>;
export function onOverlayPointerHover(handler: (hovering: boolean) => void): Promise<() => void>;
export function onOverlayPointerAt(
    handler: (point: [number, number] | null) => void,
): Promise<() => void>;
export function setOverlayInteractiveHeight(height: number): Promise<void>;
export function setOverlaySettingsOpen(open: boolean): Promise<void>;
export function setOverlayChromeSize(size: { width: number; height: number; anchor?: number }): Promise<void>;
export function dragOverlay(begin: boolean, edge?: string | null): Promise<void>;
export function onOverlaySettingsOpen(handler: (open: boolean) => void): Promise<() => void>;
export function dismissOverlayChrome(): Promise<void>;
export function openExternal(url: string): Promise<void>;
export const REPOSITORY_URL: string;
export type AutostartStatus = 'enabled' | 'disabled' | 'requiresApproval';
export function getAutostartStatus(): Promise<AutostartStatus>;
export function setAutostartEnabled(enabled: boolean): Promise<AutostartStatus>;
export function openAutostartSettings(): Promise<void>;
export function getUpdateStatus(): Promise<UpdateStatus>;
export function onUpdateStatus(handler: (status: UpdateStatus) => void): Promise<() => void>;
export function onUpdateFocus(handler: () => void): Promise<() => void>;
export function checkForUpdates(): Promise<void>;
export function setAutoCheckUpdates(enabled: boolean): Promise<void>;
export function downloadUpdate(): Promise<void>;
export function installUpdate(): Promise<void>;
