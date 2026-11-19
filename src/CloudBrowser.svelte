<script lang="ts">
  import { onMount } from 'svelte';
  import {
    Cloud,
    CloudDownload,
    CloudAlert,
    Download,
    Film,
    Music2,
    Images,
    HardDrive,
    Folder,
    File as FileIcon,
    House,
    ChevronRight,
    Search,
    X,
    Play,
    EllipsisVertical,
    Trash2,
    LoaderCircle,
    Check,
    Settings2,
    Volume2
  } from '@lucide/svelte';
  import { api, size, asMedia, thumbnail } from './lib';
  import type { CloudItem, Media, User } from './lib';

  let {
    user,
    notify,
    current,
    onopen,
    onmanage,
    onchanged
  }: {
    user: User;
    notify: (text: string) => void;
    current: Media | null;
    /** Play/view an item with the regular players. */
    onopen: (m: Media, queue: Media[]) => void;
    onmanage: () => void;
    onchanged: () => void;
  } = $props();

  type Tab = 'video' | 'audio' | 'photo' | 'file';
  let items = $state<CloudItem[]>([]);
  let loading = $state(true);
  let error = $state('');
  let connected = $state(true);
  let tab = $state<Tab>('video');
  let where = $state<'all' | 'cloud' | 'here'>('all');
  let search = $state('');
  let folder = $state('Arquivos');
  let menuFor = $state('');
  let target = $state<CloudItem | null>(null);
  let dialog = $state<'' | 'delete'>('');
  let working = $state('');

  const files = $derived(user.admin || user.can_upload);
  const tabs = $derived(
    [
      { id: 'video' as Tab, label: 'Vídeos', icon: Film },
      { id: 'audio' as Tab, label: 'Músicas', icon: Music2 },
      { id: 'photo' as Tab, label: 'Fotos', icon: Images },
      ...(files ? [{ id: 'file' as Tab, label: 'Arquivos', icon: HardDrive }] : [])
    ].map((t) => ({ ...t, count: items.filter((i) => i.kind === t.id).length }))
  );
  const shown = $derived(
    items.filter(
      (i) =>
        i.kind === tab &&
        (where === 'all' || (where === 'cloud' ? !i.here : i.here)) &&
        `${i.name} ${i.genre}`.toLocaleLowerCase().includes(search.toLocaleLowerCase())
    )
  );
  // "Arquivos" is browsed by folder, like the Files page.
  const inFolder = $derived(
    search ? shown : shown.filter((i) => i.path.slice(0, i.path.lastIndexOf('/')) === folder)
  );
  const subfolders = $derived(
    search
      ? []
      : [
          ...new Set(
            shown
              .filter((i) => i.path.startsWith(folder + '/'))
              .map((i) => i.path.slice(folder.length + 1).split('/'))
              .filter((p) => p.length > 1)
              .map((p) => p[0])
          )
        ].sort((a, b) => a.localeCompare(b))
  );
  const crumbs = $derived(folder.split('/'));
  const onlyCloud = $derived(items.filter((i) => !i.here && !i.lost).length);
  const initials = (value: string) =>
    value
      .split(/[\s._-]+/)
      .slice(0, 2)
      .map((w) => w[0])
      .join('')
      .toUpperCase();

  async function load() {
    loading = true;
    error = '';
    try {
      const [summary, list] = await Promise.all([
        api<{ connected: boolean }>('/cloud/summary'),
        api<CloudItem[]>('/cloud/items')
      ]);
      connected = summary.connected;
      items = list;
      // Start on the first tab that has something.
      if (!items.some((i) => i.kind === tab)) {
        const first = tabs.find((t) => t.count > 0);
        if (first) tab = first.id;
      }
    } catch (e) {
      error = (e as Error).message;
    } finally {
      loading = false;
    }
  }
  onMount(() => void load());

  function open(i: CloudItem) {
    menuFor = '';
    if (i.lost) {
      notify('Este arquivo foi apagado direto no Google Drive.');
      return;
    }
    if (i.kind === 'file') {
      download(i);
      return;
    }
    const queue = shown.filter((x) => !x.lost).map(asMedia);
    onopen(asMedia(i), queue);
  }
  function download(i: CloudItem) {
    const a = document.createElement('a');
    a.href = `/api/cloud/items/${i.id}/stream?download=true`;
    a.download = i.path.split('/').pop() || i.name;
    a.click();
    notify('Baixando do Google Drive para este aparelho…');
  }
  async function restore(i: CloudItem) {
    menuFor = '';
    working = i.id;
    try {
      await api('/cloud/restore', 'POST', { ids: [i.id] });
      notify(`Baixando “${i.name}” para o servidor. Acompanhe em Backup no Google Drive.`);
      // The download runs in the background; refresh when it is likely done.
      setTimeout(async () => {
        await load();
        onchanged();
      }, 4000);
    } catch (e) {
      error = (e as Error).message;
    } finally {
      working = '';
    }
  }
  async function remove() {
    if (!target) return;
    working = target.id;
    try {
      await api(`/cloud/items/${target.id}`, 'DELETE');
      notify('Movido para a lixeira do Google Drive');
      dialog = '';
      await load();
      onchanged();
    } catch (e) {
      error = (e as Error).message;
    } finally {
      working = '';
    }
  }
</script>

<svelte:window
  onclick={() => (menuFor = '')}
  onkeydown={(e) => {
    if (e.key === 'Escape' && (menuFor || dialog)) {
      e.stopImmediatePropagation();
      menuFor = '';
      dialog = '';
    }
  }}
/>

{#snippet menu(i: CloudItem)}
  <span class="cloud-menu-wrap">
    <button
      class="icon-button cloud-more"
      aria-label={`Opções de ${i.name}`}
      aria-haspopup="menu"
      aria-expanded={menuFor === i.id}
      onclick={(ev) => {
        ev.stopPropagation();
        menuFor = menuFor === i.id ? '' : i.id;
      }}
      >{#if working === i.id}<LoaderCircle size={17} class="spin" />{:else}<EllipsisVertical
          size={17}
        />{/if}</button
    >
    {#if menuFor === i.id}
      <div class="file-menu" role="menu">
        {#if !i.lost}<button role="menuitem" onclick={() => download(i)}
            ><Download size={16} />Baixar neste aparelho</button
          >{/if}
        {#if user.admin && !i.here && !i.lost}<button role="menuitem" onclick={() => restore(i)}
            ><CloudDownload size={16} />Trazer de volta ao servidor</button
          >{/if}
        {#if user.admin}<button
            role="menuitem"
            class="danger"
            onclick={() => {
              menuFor = '';
              target = i;
              dialog = 'delete';
            }}><Trash2 size={16} />Excluir do Google Drive</button
          >{/if}
      </div>
    {/if}
  </span>
{/snippet}

{#snippet badge(i: CloudItem)}
  {#if i.lost}<span class="cloud-badge lost"><CloudAlert size={11} />Apagado no Drive</span
    >{:else if i.here}<span class="cloud-badge here"><Check size={11} />Também aqui</span
    >{:else}<span class="cloud-badge"><Cloud size={11} />Só no Drive</span>{/if}
{/snippet}

<div class="cloud-banner" role="note">
  <Cloud size={20} />
  <p>
    <strong>Isto vem do Google Drive.</strong> Abrir e reproduzir demora um pouco mais: cada arquivo é
    buscado na internet pelo servidor. Para ver sem espera, traga de volta ao servidor.
  </p>
</div>

{#if error}<div class="error dismissable" role="alert">
    {error}<button class="icon-button" aria-label="Fechar erro" onclick={() => (error = '')}
      ><X size={16} /></button
    >
  </div>{/if}

{#if loading && !items.length}
  <div class="cloud-loading"><LoaderCircle class="spin" size={20} />Buscando a lista no Drive…</div>
{:else if !items.length}
  <div class="empty-state">
    <span class="empty-icon"><Cloud size={30} /></span>
    <h3>{connected ? 'Nada no Google Drive ainda.' : 'Nenhuma conta Google conectada.'}</h3>
    <p>
      {connected
        ? 'Use “Sincronizar agora” para enviar a primeira cópia da biblioteca.'
        : 'Conecte uma conta para guardar uma cópia da biblioteca fora de casa.'}
    </p>
    {#if user.admin}<button class="secondary" onclick={onmanage}
        ><Settings2 size={17} />{connected ? 'Abrir o backup' : 'Configurar backup'}</button
      >{/if}
  </div>
{:else}
  {#if !connected}
    <p class="cloud-account-note">
      <CloudAlert size={16} />A conta Google está desconectada. A lista abaixo é a última conhecida;
      para abrir os arquivos, conecte a conta de novo.
    </p>
  {/if}
  <div class="cloud-toolbar">
    <div class="cloud-tabs" role="tablist" aria-label="Tipo de arquivo">
      {#each tabs as t (t.id)}
        <button
          role="tab"
          aria-selected={tab === t.id}
          class:active={tab === t.id}
          onclick={() => {
            tab = t.id;
            folder = 'Arquivos';
          }}><t.icon size={16} />{t.label}<span class="count-pill">{t.count}</span></button
        >
      {/each}
    </div>
    <div class="library-tools">
      <label class="search-box"
        ><Search size={16} /><input
          aria-label="Pesquisar no Google Drive"
          placeholder="Buscar no Drive"
          bind:value={search}
        />{#if search}<button
            class="icon-button"
            aria-label="Limpar pesquisa"
            onclick={() => (search = '')}><X size={14} /></button
          >{/if}</label
      >
      <div class="view-toggle cloud-where-toggle" role="group" aria-label="Onde está">
        {#each [{ id: 'all', label: 'Todos' }, { id: 'cloud', label: `Só no Drive${onlyCloud ? ` (${onlyCloud})` : ''}` }, { id: 'here', label: 'Também aqui' }] as w (w.id)}
          <button
            aria-pressed={where === w.id}
            class:active={where === w.id}
            onclick={() => (where = w.id as typeof where)}>{w.label}</button
          >
        {/each}
      </div>
      {#if user.admin}<button class="secondary" onclick={onmanage}
          ><Settings2 size={16} /><span>Backup</span></button
        >{/if}
    </div>
  </div>

  {#if tab === 'file'}
    {#if !search}
      <nav class="files-crumbs cloud-crumbs" aria-label="Pasta no Drive">
        <span class="cloud-crumb-root"><House size={15} />Sonam</span>
        {#each crumbs as c, i (i)}
          <ChevronRight size={14} />
          <button
            class:current={i === crumbs.length - 1}
            onclick={() => (folder = crumbs.slice(0, i + 1).join('/'))}>{c}</button
          >
        {/each}
      </nav>
    {/if}
    {#if !subfolders.length && !inFolder.length}
      <div class="files-empty">
        <Folder size={34} /><strong>Nada aqui</strong>
        <p>
          Nenhum arquivo desta pasta foi enviado ao Drive{where !== 'all'
            ? ' com esse filtro'
            : ''}.
        </p>
      </div>
    {:else}
      <ul class="files-grid list">
        {#each subfolders as f (f)}
          <li class="folder">
            <button class="file-main" onclick={() => (folder = `${folder}/${f}`)}>
              <span class="file-thumb"><Folder size={22} /></span>
              <span class="file-text"><strong>{f}</strong><small>Pasta</small></span>
            </button>
          </li>
        {/each}
        {#each inFolder as i (i.id)}
          <li>
            <button class="file-main" onclick={() => open(i)} title={i.name}>
              <span class="file-thumb"><FileIcon size={22} /></span>
              <span class="file-text"
                ><strong>{i.name}</strong><small>{size(i.size)}{search ? ` · ${i.path}` : ''}</small
                ></span
              >
            </button>
            <span class="cloud-file-end">{@render badge(i)}{@render menu(i)}</span>
          </li>
        {/each}
      </ul>
    {/if}
  {:else if !shown.length}
    <div class="empty-state">
      <span class="empty-icon"><Search size={30} /></span>
      <h3>Nada por aqui.</h3>
      <p>
        {search
          ? 'Tente outro nome ou gênero.'
          : where === 'cloud'
            ? 'Tudo desta categoria também está neste servidor.'
            : 'Nenhum item desta categoria foi enviado ainda.'}
      </p>
    </div>
  {:else if tab === 'audio'}
    <div class="track-list">
      <div class="track-header">
        <span>#</span><span>TÍTULO</span><span>GÊNERO</span><span>NO DRIVE</span>
      </div>
      {#each shown as i, n (i.id)}
        <div class="track cloud-track" class:now-playing={current?.id === i.id} class:lost={i.lost}>
          <button class="track-number" aria-label={'Reproduzir ' + i.name} onclick={() => open(i)}
            >{#if current?.id === i.id}<Volume2 size={16} />{:else}{String(n + 1).padStart(
                2,
                '0'
              )}{/if}</button
          ><button class="track-title" onclick={() => open(i)}
            ><span class="track-art"><Music2 size={21} /></span><span
              ><strong>{i.name}</strong><small>{size(i.size)}</small></span
            ></button
          ><span class="muted">{i.genre || 'Sem gênero'}</span><span class="cloud-track-end"
            >{@render badge(i)}{@render menu(i)}</span
          >
        </div>
      {/each}
    </div>
  {:else}
    <div class="media-grid" class:photo-grid={tab === 'photo'}>
      {#each shown as i, n (i.id)}
        <article class="media-card cloud-card" class:lost={i.lost}>
          <button
            class="media-cover cover-{n % 6}"
            class:photo={i.kind === 'photo'}
            onclick={() => open(i)}
            aria-label={'Abrir ' + i.name}
            >{#if i.kind === 'photo' && !i.lost}<img
                src={thumbnail({ id: i.id, remote: true })}
                alt={i.name}
                loading="lazy"
              />{:else}<span class="cover-monogram">{initials(i.name)}</span><span
                class="cover-kind">VIA GOOGLE DRIVE</span
              ><span class="cover-play"><Play size={23} fill="currentColor" /></span>{/if}<span
              class="cloud-cover-badge">{@render badge(i)}</span
            ></button
          >
          <div class="media-info">
            <div>
              <button onclick={() => open(i)}>{i.name}</button>
              <p>{i.genre || 'Sem gênero'}<span>·</span>{size(i.size)}</p>
            </div>
            {@render menu(i)}
          </div>
        </article>
      {/each}
    </div>
  {/if}
{/if}

{#if dialog === 'delete' && target}
  <div class="modal-backdrop">
    <div
      class="modal"
      role="dialog"
      aria-modal="true"
      aria-label="Excluir do Google Drive"
      tabindex="-1"
    >
      <button class="modal-close icon-button" aria-label="Fechar" onclick={() => (dialog = '')}
        ><X size={20} /></button
      >
      <h2>Excluir “{target.name}” do Google Drive?</h2>
      <p>
        {target.here
          ? 'A cópia neste servidor continua. Na próxima sincronização ele é enviado de novo.'
          : 'Este item não existe mais neste servidor: depois disto, a única cópia fica na lixeira do Google Drive por 30 dias e some da biblioteca.'}
      </p>
      <button class="primary danger-bg" disabled={!!working} onclick={remove}
        >{#if working}<LoaderCircle size={17} class="spin" />{/if}<Trash2 size={17} />Mover para a
        lixeira do Drive</button
      >
      <button class="text-button" onclick={() => (dialog = '')}>Manter</button>
    </div>
  </div>
{/if}
