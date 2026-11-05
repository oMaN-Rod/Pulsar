<script module lang="ts">
  import bin from '$lib/assets/desk/bin.png';
  import board from '$lib/assets/desk/board.png';
  import browser from '$lib/assets/desk/browser.png';
  import chat from '$lib/assets/desk/chat.png';
  import folder from '$lib/assets/desk/folder.png';
  import mail from '$lib/assets/desk/mail.png';
  import music from '$lib/assets/desk/music.png';
  import notes from '$lib/assets/desk/notes.png';
  import open from '$lib/assets/desk/open.png';
  import photos from '$lib/assets/desk/photos.png';
  import picture from '$lib/assets/desk/picture.png';
  import save from '$lib/assets/desk/save.png';
  import settings from '$lib/assets/desk/settings.png';
  import share from '$lib/assets/desk/share.png';
  import store from '$lib/assets/desk/store.png';
  import terminal from '$lib/assets/desk/terminal.png';

  const APP_ICONS = {
    folder,
    browser,
    terminal,
    mail,
    settings,
    chat,
    music,
    photos,
    store,
    notes,
  };
  const DESK_ICONS = { bin, open, save, share, board, picture };

  export type DeskIconName =
    | 'start'
    | 'search'
    | 'taskview'
    | keyof typeof APP_ICONS
    | 'close'
    | 'chevron'
    | 'wifi'
    | 'volume'
    | 'battery'
    | 'bell'
    | keyof typeof DESK_ICONS;
</script>

<script lang="ts">
  let { name }: { name: DeskIconName } = $props();
</script>

{#if name in APP_ICONS}
  <img
    src={APP_ICONS[name as keyof typeof APP_ICONS]}
    width="24"
    height="24"
    alt=""
    draggable="false"
  />
{:else if name in DESK_ICONS}
  <img
    src={DESK_ICONS[name as keyof typeof DESK_ICONS]}
    width="40"
    height="40"
    alt=""
    draggable="false"
  />
{:else if name === 'start'}
  <svg viewBox="0 0 12 12" width="22" height="22" aria-hidden="true">
    <defs>
      <linearGradient id="st" x1="1.74" y1="0" x2="9.24" y2="11.5" gradientUnits="userSpaceOnUse">
        <stop stop-color="#4DD2FF" />
        <stop offset=".75" stop-color="#0078D4" />
      </linearGradient>
    </defs>
    <path
      d="M5.5 11.5h-5a.5.5 0 0 1-.5-.5V6h5.5zm6-.5a.5.5 0 0 1-.5.5H6V6h5.5zM5.5 5.5H0v-5A.5.5 0 0 1 .5 0h5zM11 0a.5.5 0 0 1 .5.5v5H6V0z"
      fill="url(#st)"
    />
  </svg>
{:else if name === 'search'}
  <svg viewBox="0 0 16 16" width="18" height="18" aria-hidden="true">
    <circle cx="6.8" cy="6.8" r="4.6" fill="none" stroke="currentColor" stroke-width="1.4" />
    <path d="M10.3 10.3 14 14" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
  </svg>
{:else if name === 'taskview'}
  <svg viewBox="0 0 24 24" width="24" height="24" aria-hidden="true">
    <rect x="3" y="6" width="12" height="12" rx="2.2" fill="#9CA3AF" />
    <rect
      x="9"
      y="3.5"
      width="12"
      height="12"
      rx="2.2"
      fill="#E5E7EB"
      stroke="#4B5563"
      stroke-width="0.8"
    />
  </svg>
{:else if name === 'close'}
  <svg viewBox="0 0 12 12" width="10" height="10" aria-hidden="true">
    <path d="m3 3 6 6M9 3 3 9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
  </svg>
{:else if name === 'chevron'}
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
    <path
      d="m4 10 4-4 4 4"
      fill="none"
      stroke="currentColor"
      stroke-width="1.3"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
  </svg>
{:else if name === 'wifi'}
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
    <path
      d="M1.5 6a9.5 9.5 0 0 1 13 0M3.8 8.6a6.2 6.2 0 0 1 8.4 0M6.1 11.1a2.8 2.8 0 0 1 3.8 0"
      fill="none"
      stroke="currentColor"
      stroke-width="1.3"
      stroke-linecap="round"
    />
    <circle cx="8" cy="13.4" r=".9" fill="currentColor" />
  </svg>
{:else if name === 'volume'}
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
    <path
      d="M2 6h2.6L8 3.2v9.6L4.6 10H2z"
      fill="none"
      stroke="currentColor"
      stroke-width="1.2"
      stroke-linejoin="round"
    />
    <path
      d="M10.6 5.6a3.4 3.4 0 0 1 0 4.8M12.4 3.9a5.8 5.8 0 0 1 0 8.2"
      fill="none"
      stroke="currentColor"
      stroke-width="1.2"
      stroke-linecap="round"
    />
  </svg>
{:else if name === 'battery'}
  <svg viewBox="0 0 20 16" width="20" height="16" aria-hidden="true">
    <rect
      x="1.5"
      y="4"
      width="15"
      height="8.5"
      rx="2"
      fill="none"
      stroke="currentColor"
      stroke-width="1.2"
    />
    <rect x="3.2" y="5.7" width="10.5" height="5.1" rx=".8" fill="currentColor" />
    <path d="M17.8 6.6v3.3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
  </svg>
{:else if name === 'bell'}
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
    <path
      d="M4 11V7.3a4 4 0 0 1 8 0V11l1.2 1.3H2.8zM6.5 13.6a1.6 1.6 0 0 0 3 0"
      fill="none"
      stroke="currentColor"
      stroke-width="1.2"
      stroke-linejoin="round"
    />
  </svg>
{/if}
