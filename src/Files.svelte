<script lang="ts">
  import { onMount } from 'svelte';
  import {
    Folder,
    FolderPlus,
    File as FileIcon,
    FileText,
    FileArchive,
    FileMusic,
    FileVideoCamera,
    Upload,
    EllipsisVertical,
    FolderInput,
    Trash2,
    Pencil,
    House,
    ChevronRight,
    LayoutGrid,
    List,
    Music2,
    Film,
    Images,
    Download,
    X,
    Check,
    LoaderCircle,
    HardDrive,
    ArrowLeft
  } from '@lucide/svelte';
  import { api, size } from './lib';
  import type { DriveEntry, DriveListing } from './lib';

  let {
    notify,
    onopen,
    onchanged
  }: {
    notify: (text: string) => void;
    /** Open a library item (photo/video/music) with its folder siblings as queue. */
    onopen: (id: string, siblings: string[]) => void;
    onchanged: () => void;
  } = $props();

  const readView = () => {
    try {
      return localStorage.getItem('hs-files-view') === 'list' ? 'list' : 'grid';
    } catch {
      return 'grid';
    }
  };
  let path = $state('');
  let listing = $state<DriveListing | null>(null);
  let loading = $state(true);
  let error = $state('');
  let view = $state<'grid' | 'list'>(readView());
  let menuFor = $state('');
  let dialog = $state<'' | 'folder' | 'rename' | 'move' | 'delete'>('');
  let target = $state<DriveEntry | null>(null);
  let nameInput = $state('');
  let working = $state(false);
  let dragging = $state(false);
  let uploads = $state<{ name: string; progress: number }[]>([]);
  let fileInput = $state<HTMLInputElement>();
  // Move dialog has its own little browser.
  let movePath = $state('');
  let moveFolders = $state<DriveEntry[]>([]);

  const TOP: Record<string, { icon: typeof Folder; hint: string }> = {
    Fotos: { icon: Images, hint: 'Entram na biblioteca de fotos' },
    Músicas: { icon: Music2, hint: 'Entram na biblioteca de músicas' },
    Vídeos: { icon: Film, hint: 'Entram em filmes e vídeos' },
    Arquivos: { icon: HardDrive, hint: 'Documentos, backups e o resto' },
    Biblioteca: { icon: Folder, hint: 'Pasta da versão anterior' }
  };

  const crumbs = $derived(path ? path.split('/') : []);
  // At the top, show the library folders first, in a fixed order.
  const ORDER = ['Fotos', 'Músicas', 'Vídeos', 'Arquivos'];
  const rank = (e: DriveEntry) => (path ? 0 : ((i) => (i < 0 ? 99 : i))(ORDER.indexOf(e.name)));
  const entries = $derived([...(listing?.entries ?? [])].sort((a, b) => rank(a) - rank(b)));
  const folders = $derived(entries.filter((e) => e.dir));
  const files = $derived(entries.filter((e) => !e.dir));

  const davUrl = (p: string) => '/dav/' + p.split('/').map(encodeURIComponent).join('/');
  const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);
  const parent = (p: string) => p.split('/').slice(0, -1).join('/');
  const ext = (n: string) => n.split('.').pop()?.toLowerCase() ?? '';
  const when = (s: number) =>
    s ? new Intl.DateTimeFormat('pt-BR', { dateStyle: 'medium' }).format(s * 1000) : '';
  function iconFor(e: DriveEntry) {
    if (e.dir) return TOP[e.name] && !e.path.includes('/') ? TOP[e.name].icon : Folder;
    const k = e.media?.kind;
    if (k === 'audio' || ['mp3', 'flac', 'wav', 'ogg', 'm4a', 'aac'].includes(ext(e.name)))
      return FileMusic;
    if (k === 'video' || ['mp4', 'mkv', 'webm', 'mov', 'avi'].includes(ext(e.name)))
      return FileVideoCamera;
    if (['zip', 'rar', '7z', 'tar', 'gz'].includes(ext(e.name))) return FileArchive;
    if (['txt', 'pdf', 'doc', 'docx', 'md', 'odt', 'xls', 'xlsx', 'csv'].includes(ext(e.name)))
      return FileText;
    return FileIcon;
  }

  async function load(next = path) {
    loading = true;
    error = '';
    menuFor = '';
    try {
      listing = await api<DriveListing>(`/drive/list?path=${encodeURIComponent(next)}`);
      path = listing.path;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      loading = false;
    }
  }
  onMount(() => void load(''));

  async function dav(method: string, p: string, headers: Record<string, string> = {}) {
    const r = await fetch(davUrl(p), { method, headers, credentials: 'same-origin' });
    if (!r.ok) {
      const messages: Record<number, string> = {
        403: 'Esta pasta não pode ser alterada.',
        405: 'Já existe um item com esse nome.',
        409: 'A pasta de destino não existe mais.',
        412: 'Já existe um item com esse nome no destino.',
        507: 'O servidor está sem espaço livre.'
      };
      throw new Error(messages[r.status] || `Não foi possível concluir (erro ${r.status}).`);
    }
  }

  // After changes in Fotos/Músicas/Vídeos the library must pick them up now,
  // not on the next background check.
  async function refreshLibrary() {
    try {
      await api('/drive/scan', 'POST');
    } catch {
      /* the background scanner catches up */
    }
    onchanged();
  }

  async function run(fn: () => Promise<void>, done: string) {
    working = true;
    error = '';
    try {
      await fn();
      dialog = '';
      notify(done);
      await load();
      void refreshLibrary();
    } catch (e) {
      error = (e as Error).message;
    } finally {
      working = false;
    }
  }

  const invalidName = (n: string) =>
    !n.trim() || /[\\/:*?"<>|]/.test(n) || n.trim().startsWith('.') || n.length > 200;

  function openDialog(kind: typeof dialog, e: DriveEntry | null = null) {
    menuFor = '';
    target = e;
    error = '';
    nameInput = kind === 'rename' && e ? e.name : '';
    dialog = kind;
    if (kind === 'move' && e) void browseMove(parent(e.path));
  }

  async function browseMove(p: string) {
    try {
      const l = await api<DriveListing>(`/drive/list?path=${encodeURIComponent(p)}`);
      movePath = l.path;
      moveFolders = l.entries.filter((x) => x.dir && x.path !== target?.path);
    } catch (e) {
      error = (e as Error).message;
    }
  }

  function uniqueName(name: string) {
    const taken = new Set(entries.map((e) => e.name.toLowerCase()));
    if (!taken.has(name.toLowerCase())) return name;
    const dot = name.lastIndexOf('.');
    const stem = dot > 0 ? name.slice(0, dot) : name;
    const extension = dot > 0 ? name.slice(dot) : '';
    for (let n = 2; ; n++) {
      const candidate = `${stem} (${n})${extension}`;
      if (!taken.has(candidate.toLowerCase())) return candidate;
    }
  }

  async function upload(list: FileList | File[]) {
    const all = Array.from(list);
    if (!all.length || !path) return;
    const dir = path;
    uploads = all.map((f) => ({ name: f.name, progress: 0 }));
    let ok = 0;
    error = '';
    for (const [i, f] of all.entries()) {
      const name = uniqueName(f.name.replace(/[\\/:*?"<>|]/g, '_'));
      try {
        await new Promise<void>((resolve, reject) => {
          const xhr = new XMLHttpRequest();
          xhr.open('PUT', davUrl(join(dir, name)));
          xhr.upload.onprogress = (e) => {
            if (e.lengthComputable) uploads[i].progress = (e.loaded / e.total) * 100;
          };
          xhr.onload = () =>
            xhr.status >= 200 && xhr.status < 300
              ? resolve()
              : reject(
                  new Error(xhr.status === 507 ? 'Servidor sem espaço livre' : `Erro ${xhr.status}`)
                );
          xhr.onerror = () => reject(new Error('Conexão interrompida durante o envio'));
          xhr.send(f);
        });
        uploads[i].progress = 100;
        ok++;
        if (dir === path) await load();
      } catch (e) {
        error = `${f.name}: ${(e as Error).message}`;
        break;
      }
    }
    uploads = [];
    if (fileInput) fileInput.value = '';
    if (ok) {
      notify(`${ok} arquivo${ok > 1 ? 's' : ''} guardado${ok > 1 ? 's' : ''}`);
      void refreshLibrary();
    }
  }

  function activate(e: DriveEntry) {
    if (e.dir) {
      void load(e.path);
      return;
    }
    if (e.media) {
      onopen(
        e.media.id,
        files.filter((f) => f.media?.kind === e.media?.kind).map((f) => f.media!.id)
      );
      return;
    }
    const a = document.createElement('a');
    a.href = davUrl(e.path);
    a.download = e.name;
    a.click();
  }

  const protectedTop = (e: DriveEntry) => !e.path.includes('/') && e.name in TOP;
  const isWatched = $derived(listing?.watched ?? false);
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

<section
  class="files"
  class:dragging
  aria-label="Arquivos do servidor"
  ondragover={(e) => {
    if (!path || !e.dataTransfer?.types.includes('Files')) return;
    e.preventDefault();
    dragging = true;
  }}
  ondragleave={(e) => {
    if (e.currentTarget === e.target) dragging = false;
  }}
  ondrop={(e) => {
    e.preventDefault();
    dragging = false;
    if (e.dataTransfer?.files.length) void upload(e.dataTransfer.files);
  }}
>
  <div class="files-toolbar">
    <nav class="files-crumbs" aria-label="Pasta atual">
      <button class:current={!path} onclick={() => load('')}
        ><House size={16} /><span>Início</span></button
      >
      {#each crumbs as c, i (i)}
        <ChevronRight size={14} />
        <button
          class:current={i === crumbs.length - 1}
          aria-current={i === crumbs.length - 1 ? 'page' : undefined}
          onclick={() => load(crumbs.slice(0, i + 1).join('/'))}>{c}</button
        >
      {/each}
    </nav>
    <div class="files-actions">
      {#if path}
        <button class="secondary" onclick={() => openDialog('folder')}
          ><FolderPlus size={17} /><span>Nova pasta</span></button
        >
        <button class="primary" onclick={() => fileInput?.click()}
          ><Upload size={17} /><span>Enviar</span></button
        >
      {/if}
      <div class="view-toggle" role="group" aria-label="Modo de exibição">
        <button
          aria-label="Grade"
          aria-pressed={view === 'grid'}
          class:active={view === 'grid'}
          onclick={() => {
            view = 'grid';
            try {
              localStorage.setItem('hs-files-view', 'grid');
            } catch {
              /* ignore */
            }
          }}><LayoutGrid size={17} /></button
        >
        <button
          aria-label="Lista"
          aria-pressed={view === 'list'}
          class:active={view === 'list'}
          onclick={() => {
            view = 'list';
            try {
              localStorage.setItem('hs-files-view', 'list');
            } catch {
              /* ignore */
            }
          }}><List size={17} /></button
        >
      </div>
    </div>
    <input
      bind:this={fileInput}
      type="file"
      multiple
      hidden
      onchange={(e) => e.currentTarget.files && upload(e.currentTarget.files)}
    />
  </div>

  {#if path && isWatched}
    <p class="files-note">
      <Check size={15} /> Fotos, músicas e vídeos desta pasta aparecem também na biblioteca. Subpastas
      viram o gênero.
    </p>
  {/if}
  {#if error && !dialog}<div class="error dismissable" role="alert">
      {error}<button class="icon-button" aria-label="Fechar erro" onclick={() => (error = '')}
        ><X size={16} /></button
      >
    </div>{/if}

  {#if uploads.length}
    <div class="files-uploads" role="status">
      {#each uploads as u (u.name)}
        <div>
          <span>{u.name}</span><progress max="100" value={u.progress}></progress>
        </div>
      {/each}
    </div>
  {/if}

  {#if loading && !listing}
    <div class="files-grid skeleton" aria-label="Carregando">
      {#each Array(6) as _, i (i)}<span></span>{/each}
    </div>
  {:else if !entries.length}
    <div class="files-empty">
      <Folder size={34} />
      <strong>Pasta vazia</strong>
      <p>Arraste arquivos para cá ou use “Enviar”. Também dá para criar subpastas.</p>
    </div>
  {:else}
    <ul class="files-grid" class:list={view === 'list'} class:busy={loading}>
      {#each entries as e (e.path)}
        {@const Icon = iconFor(e)}
        <li class:folder={e.dir} class:top={!path && e.dir}>
          <button class="file-main" onclick={() => activate(e)} title={e.name}>
            <span class="file-thumb" class:has-image={e.media?.kind === 'photo'}>
              {#if e.media?.kind === 'photo'}
                <img src={`/api/media/${e.media.id}/thumbnail`} alt="" loading="lazy" />
              {:else}
                <Icon size={view === 'list' ? 22 : !path && e.dir ? 34 : 30} />
              {/if}
            </span>
            <span class="file-text">
              <strong>{e.name}</strong>
              <small
                >{#if !path && TOP[e.name]}{TOP[e.name].hint}{:else if e.dir}Pasta{:else}{size(
                    e.size
                  )}{e.modified ? ` · ${when(e.modified)}` : ''}{/if}</small
              >
            </span>
          </button>
          {#if !protectedTop(e)}
            <button
              class="icon-button file-more"
              aria-label={`Opções de ${e.name}`}
              aria-haspopup="menu"
              aria-expanded={menuFor === e.path}
              onclick={(ev) => {
                ev.stopPropagation();
                menuFor = menuFor === e.path ? '' : e.path;
              }}><EllipsisVertical size={18} /></button
            >
            {#if menuFor === e.path}
              <div class="file-menu" role="menu">
                {#if !e.dir}<a role="menuitem" href={davUrl(e.path)} download={e.name}
                    ><Download size={16} />Baixar</a
                  >{/if}
                <button role="menuitem" onclick={() => openDialog('rename', e)}
                  ><Pencil size={16} />Renomear</button
                >
                <button role="menuitem" onclick={() => openDialog('move', e)}
                  ><FolderInput size={16} />Mover para…</button
                >
                <button role="menuitem" class="danger" onclick={() => openDialog('delete', e)}
                  ><Trash2 size={16} />Excluir</button
                >
              </div>
            {/if}
          {/if}
        </li>
      {/each}
    </ul>
    {#if listing && listing.free !== null && !path}
      <p class="files-space">
        <HardDrive size={15} />{size(listing.free)} livres{listing.total
          ? ` de ${size(listing.total)}`
          : ''} no armazenamento do servidor
      </p>
    {/if}
  {/if}
  {#if dragging}<div class="files-drop" aria-hidden="true">
      <Upload size={30} /><strong>Solte para guardar em {crumbs.at(-1)}</strong>
    </div>{/if}
</section>

{#if dialog}
  <div class="modal-backdrop">
    <div class="modal" role="dialog" aria-modal="true" aria-label="Arquivos" tabindex="-1">
      <button class="modal-close icon-button" aria-label="Fechar" onclick={() => (dialog = '')}
        ><X size={20} /></button
      >
      {#if dialog === 'folder' || dialog === 'rename'}
        <h2>{dialog === 'folder' ? 'Nova pasta' : 'Renomear'}</h2>
        <form
          onsubmit={(ev) => {
            ev.preventDefault();
            const name = nameInput.trim();
            if (invalidName(name)) {
              error = 'Use um nome sem / \\ : * ? " < > | e que não comece com ponto.';
              return;
            }
            if (dialog === 'folder') void run(() => dav('MKCOL', join(path, name)), 'Pasta criada');
            else if (target && name !== target.name) {
              const from = target.path;
              void run(
                () =>
                  dav('MOVE', from, {
                    Destination: location.origin + davUrl(join(parent(from), name)),
                    Overwrite: 'F'
                  }),
                'Nome alterado'
              );
            } else dialog = '';
          }}
        >
          <label
            >Nome<input
              required
              maxlength="200"
              bind:value={nameInput}
              placeholder={dialog === 'folder' ? 'Viagem 2026' : ''}
              autocomplete="off"
            /></label
          >
          {#if error}<div class="error" role="alert">{error}</div>{/if}
          <button class="primary" disabled={working}
            >{#if working}<LoaderCircle size={17} class="spin" />{/if}{dialog === 'folder'
              ? 'Criar pasta'
              : 'Salvar nome'}</button
          >
        </form>
      {:else if dialog === 'move' && target}
        <h2>Mover “{target.name}”</h2>
        <p>Escolha a pasta de destino.</p>
        <div class="move-browser">
          <div class="move-crumbs">
            {#if movePath}<button
                class="icon-button"
                aria-label="Voltar uma pasta"
                onclick={() => browseMove(parent(movePath))}><ArrowLeft size={17} /></button
              >{/if}
            <strong>{movePath || 'Início'}</strong>
          </div>
          <ul>
            {#each moveFolders as f (f.path)}
              <li>
                <button onclick={() => browseMove(f.path)}
                  ><Folder size={18} />{f.name}<ChevronRight size={16} /></button
                >
              </li>
            {:else}
              <li class="muted small">Nenhuma subpasta aqui.</li>
            {/each}
          </ul>
        </div>
        {#if error}<div class="error" role="alert">{error}</div>{/if}
        <button
          class="primary"
          disabled={working ||
            !movePath ||
            movePath === parent(target.path) ||
            movePath.startsWith(target.path + '/')}
          onclick={() => {
            const from = target!.path;
            const dest = join(movePath, target!.name);
            void run(
              () =>
                dav('MOVE', from, {
                  Destination: location.origin + davUrl(dest),
                  Overwrite: 'F'
                }),
              'Movido'
            );
          }}
          >{#if working}<LoaderCircle size={17} class="spin" />{/if}Mover para {movePath ||
            'Início'}</button
        >
        {#if !movePath}<p class="muted small">
            Escolha uma das pastas: arquivos não ficam soltos no início.
          </p>{/if}
      {:else if dialog === 'delete' && target}
        <h2>Excluir “{target.name}”?</h2>
        <p>
          {target.dir
            ? 'A pasta e tudo o que estiver dentro dela serão apagados do servidor.'
            : 'O arquivo será apagado do servidor.'}
          {isWatched ? 'Se estiver na biblioteca, sai dela também.' : ''} Esta ação é permanente.
        </p>
        {#if error}<div class="error" role="alert">{error}</div>{/if}
        <button
          class="primary danger-bg"
          disabled={working}
          onclick={() => {
            const p = target!.path;
            void run(() => dav('DELETE', p), 'Excluído');
          }}
          >{#if working}<LoaderCircle size={17} class="spin" />{/if}<Trash2 size={17} />Excluir
          permanentemente</button
        >
        <button class="text-button" onclick={() => (dialog = '')}>Cancelar</button>
      {/if}
    </div>
  </div>
{/if}
