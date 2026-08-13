<script lang="ts">
    let { name, size = 18 }: { name: string; size?: number } = $props();
</script>

<svg
    class="icon"
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
>
    {#if name === 'settings'}
        <circle cx="12" cy="12" r="3"></circle>
        <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 .6 1.7 1.7 0 0 0-.4 1.1V21h-4v-.09A1.7 1.7 0 0 0 8.6 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-.6-1 1.7 1.7 0 0 0-1.1-.4H3v-4h.09A1.7 1.7 0 0 0 4.6 8.6a1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-.6 1.7 1.7 0 0 0 .4-1.1V3h4v.09A1.7 1.7 0 0 0 15.4 4.6a1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.4 9c.13.38.35.72.66.98.3.26.7.4 1.1.4H21v4h-.09a1.7 1.7 0 0 0-1.51.62Z"></path>
    {:else if name === 'play'}
        <path d="m9 7 7 5-7 5V7Z"></path>
    {:else if name === 'pause'}
        <path d="M9 8v8M15 8v8"></path>
    {:else if name === 'resume'}
        <path d="m10 8 6 4-6 4V8Z"></path>
    {:else if name === 'stop'}
        <rect x="8" y="8" width="8" height="8" rx="1"></rect>
    {:else if name === 'overlay'}
        <rect x="3" y="4" width="18" height="14" rx="2"></rect>
        <path d="M7 9h10M7 13h7M9 21h6M12 18v3"></path>
    {:else if name === 'home'}
        <path d="m4 11 8-7 8 7"></path>
        <path d="M6.5 9.5V20h11V9.5M10 20v-6h4v6"></path>
    {:else if name === 'pin'}
        <path d="M8 3h8M9 3v6l-3 6h12l-3-6V3M12 15v6"></path>
    {:else if name === 'retry'}
        <path d="M20 11a8 8 0 1 0-2.3 5.7M20 5v6h-6"></path>
    {:else if name === 'external'}
        <path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"></path>
    {:else if name === 'swap'}
        <path d="M7 7h11l-3-3M17 17H6l3 3"></path>
    {:else if name === 'trash'}
        <path d="M4 7h16M9 7V4h6v3M7 7l1 13h8l1-13M10 11v5M14 11v5"></path>
    {:else if name === 'export'}
        <path d="M12 4v11M8 11l4 4 4-4"></path>
        <path d="M5 17v3h14v-3"></path>
    {:else if name === 'rename'}
        <path d="m4 16 10-10 4 4-10 10H4v-4Z"></path>
        <path d="m12 8 4 4"></path>
    {:else if name === 'clear'}
        <path d="m4 15 9-9 5 5-9 9H4v-5Z"></path>
        <path d="m11 8 5 5M13 20h7"></path>
    {:else if name === 'speaker'}
        <path d="M5 10v4h3l4 3V7L8 10H5Z"></path>
        <path d="M16 9.5a4 4 0 0 1 0 5M18.5 7a7 7 0 0 1 0 10"></path>
    {:else if name === 'mic'}
        <rect x="9" y="3" width="6" height="11" rx="3"></rect>
        <path d="M6 11a6 6 0 0 0 12 0M12 17v4M9 21h6"></path>
    {:else if name === 'check'}
        <path d="m6 12 4 4 8-9"></path>
    {:else if name === 'close'}
        <path d="m7 7 10 10M17 7 7 17"></path>
    {:else if name === 'history'}
        <path d="M4 12a8 8 0 1 0 2.3-5.7L4 8.6M4 4v4.6h4.6"></path>
        <path d="M12 8v4l3 2"></path>
    {/if}
</svg>
