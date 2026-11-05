<script lang="ts">
  import { app } from '../../state.svelte.ts';
  import { META, resolve, type ItemKey } from '../../theme.ts';
  import Picker from '../Picker.svelte';

  const CHECKS = [
    { id: 'taskbar', label: 'Taskbar' },
    { id: 'widget', label: 'Widget' },
  ];

  const dark = $derived(resolve(app.theme, false));
  const options = $derived(
    app.profile.order.map((k) => ({
      key: k,
      name: META[k].name,
      color: dark.items[k],
      checked: [app.profile.items[k].taskbar, app.profile.items[k].widget],
    })),
  );

  function toggle(key: ItemKey, where: string) {
    const item = app.profile.items[key];
    if (where === 'taskbar') item.taskbar = !item.taskbar;
    else item.widget = !item.widget;
  }
</script>

<Picker
  label="Item"
  {options}
  selected={app.selected}
  onpick={(k) => (app.selected = k)}
  checks={CHECKS}
  ontoggle={toggle}
/>
