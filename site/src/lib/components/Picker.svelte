<script module lang="ts">
  export interface PickerOption<T> {
    key: T;
    name: string;
    color: string;
    note?: string;
    checked?: boolean[];
  }
</script>

<script lang="ts" generics="K extends string">
  import { tick } from 'svelte';

  /**
   * A select-only combobox whose options carry a color dot, which a native
   * <select> cannot draw, and optional checkboxes that toggle in place.
   * Follows the ARIA listbox pattern; Space and Shift+Space flip the first and
   * second checkbox of the active option.
   */
  let {
    label,
    options,
    selected,
    onpick,
    checks = [],
    ontoggle,
  }: {
    label: string;
    options: PickerOption<K>[];
    selected: K;
    onpick: (key: K) => void;
    checks?: { id: string; label: string }[];
    ontoggle?: (key: K, where: string) => void;
  } = $props();

  const uid = $props.id();
  const id = `picker-${uid}`;
  let open = $state(false);
  let active = $state(0);
  let host: HTMLElement;
  let button: HTMLButtonElement;
  let list = $state<HTMLUListElement>();

  const current = $derived(options.find((o) => o.key === selected));
  const optionId = (i: number) => `${id}-opt-${i}`;

  function setActive(i: number) {
    active = Math.max(0, Math.min(options.length - 1, i));
    tick().then(() =>
      list?.querySelector(`#${optionId(active)}`)?.scrollIntoView({ block: 'nearest' }),
    );
  }

  async function show() {
    open = true;
    setActive(
      Math.max(
        0,
        options.findIndex((o) => o.key === selected),
      ),
    );
    await tick();
    list?.focus();
  }

  function close(refocus = true) {
    if (!open) return;
    open = false;
    if (refocus) button.focus();
  }

  function choose(i: number) {
    const o = options[i];
    close();
    if (o && o.key !== selected) onpick(o.key);
  }

  function toggle(i: number, which: number) {
    const o = options[i];
    if (o && checks[which]) ontoggle?.(o.key, checks[which].id);
  }

  function buttonKey(e: KeyboardEvent) {
    if (['ArrowDown', 'ArrowUp', 'Enter', ' '].includes(e.key)) {
      e.preventDefault();
      show();
    }
  }

  function listClick(e: MouseEvent) {
    const li = (e.target as Element).closest<HTMLElement>('[role=option]');
    if (!li) return;
    const i = Number(li.dataset.index);
    const box = (e.target as Element).closest<HTMLElement>('[data-check]');
    if (box) {
      setActive(i);
      toggle(i, Number(box.dataset.check));
      list?.focus();
    } else choose(i);
  }

  function listKey(e: KeyboardEvent) {
    const step = ({ ArrowDown: 1, ArrowUp: -1, PageDown: 5, PageUp: -5 } as Record<string, number>)[
      e.key
    ];
    if (step) setActive(active + step);
    else if (e.key === 'Home') setActive(0);
    else if (e.key === 'End') setActive(options.length - 1);
    else if (e.key === ' ' && checks.length) toggle(active, e.shiftKey ? 1 : 0);
    else if (e.key === 'Enter' || e.key === ' ') choose(active);
    else if (e.key === 'Escape' || e.key === 'Tab') close(e.key === 'Escape');
    else if (e.key.length === 1) {
      const from = (active + 1) % options.length;
      const order = [...options.slice(from), ...options.slice(0, from)];
      const hit = order.find((o) => o.name.toLowerCase().startsWith(e.key.toLowerCase()));
      if (hit) setActive(options.indexOf(hit));
      return;
    } else return;
    e.preventDefault();
  }
</script>

<svelte:document
  onpointerdown={(e) => {
    if (!host.contains(e.target as Node)) close(false);
  }}
/>

<div class={['field', 'picker', checks.length > 0 && 'with-checks']} bind:this={host}>
  <span class="field-name" id="{id}-label">{label}</span>
  <button
    type="button"
    class="picker-button"
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-labelledby="{id}-label {id}-value"
    bind:this={button}
    onclick={() => (open ? close() : show())}
    onkeydown={buttonKey}
  >
    <span class="dot picker-dot" style:background={current?.color ?? 'transparent'}></span>
    <span class="picker-value" id="{id}-value">{current?.name ?? ''}</span>
    <svg class="picker-chevron" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
      <path
        d="m4 6 4 4 4-4"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
  </button>
  {#if checks.length}
    <span class="sr-only" id="{id}-hint">
      Enter edits the item. {checks
        .map(
          (c, i) =>
            `${i ? 'Shift+Space' : 'Space'} shows or hides it on the ${c.label.toLowerCase()}.`,
        )
        .join(' ')}
    </span>
  {/if}
  <div class="picker-pop" hidden={!open}>
    {#if checks.length}
      <div class="picker-head" aria-hidden="true">
        <span></span>
        {#each checks as c (c.id)}
          <span>{c.label}</span>
        {/each}
      </div>
    {/if}
    <ul
      class="picker-list"
      role="listbox"
      tabindex="-1"
      aria-labelledby="{id}-label"
      aria-describedby={checks.length ? `${id}-hint` : null}
      aria-activedescendant={open ? optionId(active) : null}
      bind:this={list}
      onkeydown={listKey}
      onclick={listClick}
    >
      {#each options as o, i (o.key)}
        <li
          role="option"
          id={optionId(i)}
          aria-selected={o.key === selected}
          aria-label={checks.length
            ? `${o.name}, ${checks.map((c, j) => `${o.checked?.[j] ? 'on' : 'off'} the ${c.label.toLowerCase()}`).join(', ')}`
            : null}
          class={[open && i === active && 'active']}
          data-index={i}
          onpointermove={() => active !== i && setActive(i)}
        >
          <span class="dot" style:background={o.color}></span>
          <span class="picker-name">{o.name}</span>
          {#if o.note}
            <code>{o.note}</code>
          {/if}
          {#each checks as c, j (c.id)}
            <span class="pk-box" data-check={j} data-on={!!o.checked?.[j]}></span>
          {/each}
        </li>
      {/each}
    </ul>
  </div>
</div>
