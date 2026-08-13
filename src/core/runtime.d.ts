import type { ControllerAction } from '../app/types';

export function invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T>;
export function subscribeController<T>(
    surface: 'main' | 'overlay',
    handler: (snapshot: T) => void,
): Promise<() => void>;
export function sendControllerAction(request: ControllerAction): Promise<void>;
export function showSettingsWindow(): Promise<void>;
export function onOverlayOutsideClick(handler: () => void): Promise<() => void>;
export function onOverlayPointerHover(handler: (hovering: boolean) => void): Promise<() => void>;
export function setOverlayInteractiveHeight(height: number): Promise<void>;
export function openExternal(url: string): Promise<void>;
export function getAutostartEnabled(): Promise<boolean>;
export function setAutostartEnabled(enabled: boolean): Promise<void>;
