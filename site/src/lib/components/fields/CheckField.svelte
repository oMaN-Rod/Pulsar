<script lang="ts">
  let {
    name,
    checked = $bindable(),
    disabled = false,
    class: cls,
  }: { name: string; checked: boolean; disabled?: boolean; class?: string } = $props();
</script>

<label class="check">
  <input type="checkbox" class={cls} bind:checked {disabled} />
  <span>{name}</span>
</label>
