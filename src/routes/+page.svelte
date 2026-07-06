<script lang="ts">
  import { api } from "$lib/api";
  import { app, refreshTree, settings } from "$lib/state.svelte";
  import { applyTheme } from "$lib/theme.svelte";
  import Onboarding from "$lib/components/Onboarding.svelte";
  import Workspace from "$lib/components/Workspace.svelte";

  let ready = $state(false);
  let needsOnboarding = $state(true);

  $effect(() => {
    (async () => {
      app.config = await api.getConfig();
      await applyTheme(settings().themeId ?? "builtin:dark", false).catch(() => {});
      needsOnboarding = !app.config.onboarding_done || !app.config.workspace_path;
      if (!needsOnboarding) await refreshTree().catch(() => {});
      ready = true;
    })();
  });

  async function onOnboarded() {
    needsOnboarding = false;
    await refreshTree().catch(() => {});
  }
</script>

{#if !ready}
  <div class="boot">🐾</div>
{:else if needsOnboarding}
  <Onboarding onDone={onOnboarded} />
{:else}
  <Workspace />
{/if}

<style>
  .boot {
    height: 100vh;
    display: grid;
    place-items: center;
    font-size: 48px;
  }
</style>
