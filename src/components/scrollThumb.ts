import type { Action } from 'svelte/action';

// A thin thumb beside a scroller whose native scrollbar is hidden (WKWebView cannot make
// its own thin). Goes on a wrapper around the scroller, so the thumb sits outside the
// scrolled content; CSS places it from the two fractions set here. Direct DOM writes on
// purpose: this runs on every scroll frame and must not go through the reactive graph.
export const scrollThumb: Action<HTMLElement> = host => {
    const scroller = host.firstElementChild as HTMLElement;
    const thumb = host.appendChild(document.createElement('i'));
    thumb.className = 'scroll-thumb';
    let range = 1;
    let hide: ReturnType<typeof setTimeout> | undefined;

    const place = () => host.style.setProperty('--scroll-top', String(scroller.scrollTop / range));
    const measure = () => {
        range = scroller.scrollHeight;
        host.style.setProperty('--scroll-size', String(scroller.clientHeight / range));
        place();
    };
    const reveal = () => {
        place();
        thumb.classList.add('visible');
        clearTimeout(hide);
        hide = setTimeout(() => thumb.classList.remove('visible'), 700);
    };

    const observer = new ResizeObserver(measure);
    observer.observe(scroller);
    for (const child of scroller.children) observer.observe(child); // the content blocks are static
    scroller.addEventListener('scroll', reveal, { passive: true });
    return {
        destroy() {
            observer.disconnect();
            clearTimeout(hide);
            scroller.removeEventListener('scroll', reveal);
            thumb.remove();
        },
    };
};
