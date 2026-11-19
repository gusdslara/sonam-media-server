<script lang="ts">
  import { onMount, tick } from 'svelte';
  import {
    House,
    Film,
    Music2,
    Images,
    Library,
    Search,
    Plus,
    Upload,
    Play,
    Pause,
    SkipForward,
    SkipBack,
    Shuffle,
    Repeat2,
    X,
    Settings as SettingsIcon,
    ChevronRight,
    ArrowUpRight,
    Monitor,
    ShieldCheck,
    HardDrive,
    LogOut,
    FolderPlus,
    MoreHorizontal,
    Check,
    Wifi,
    Smartphone,
    Tv,
    Volume2,
    Maximize,
    ArrowDownUp,
    LoaderCircle,
    Link,
    Users,
    Pencil,
    Trash2,
    ArrowLeft,
    CloudOff,
    Cloud,
    CloudCheck,
    CloudUpload
  } from '@lucide/svelte';
  import QRCode from 'qrcode';
  import { api, source, size, clock, labels } from './lib';
  import type { User, Media, Collection, Settings, Connect } from './lib';
  import Addresses from './Addresses.svelte';
  import NetworkDrive from './NetworkDrive.svelte';
  import PhotoViewer from './PhotoViewer.svelte';
  import Files from './Files.svelte';
  import CloudBackup from './CloudBackup.svelte';
  import CloudBrowser from './CloudBrowser.svelte';
  import { FolderOpen } from '@lucide/svelte';

  let ready = $state(false),
    setup = $state(false),
    local = $state(false),
    serverName = $state(''),
    serverPort = $state(8096),
    user = $state<User | null>(null);
  let name = $state(''),
    password = $state(''),
    setupToken = $state('');
  let error = $state(''),
    toast = $state(''),
    busy = $state(false);
  let items = $state<Media[]>([]),
    collections = $state<Collection[]>([]),
    profiles = $state<User[]>([]);
  let page = $state('home'),
    search = $state(''),
    sort = $state('recent'),
    selectedCollection = $state('');
  let modal = $state(''),
    settings = $state<Settings | null>(null),
    connectInfo = $state<Connect | null>(null),
    cloudSummary = $state<{ connected: boolean; items: number } | null>(null),
    cloudKey = $state(0);
  let uploadInput: HTMLInputElement,
    uploadProgress = $state(-1),
    uploadName = $state('');
  let collectionName = $state(''),
    collectionKind = $state<'video' | 'audio' | 'photo'>('video');
  let current = $state<Media | null>(null),
    queue = $state<Media[]>([]),
    playing = $state(false),
    position = $state(0),
    duration = $state(0);
  let mediaElement = $state<HTMLMediaElement>(),
    lastSaved = 0;
  let compatible = $state(false),
    forceCpu = $state(false),
    offset = $state(0);
  const playbackSource = (m: Media) =>
    compatible ? `/api/media/${m.id}/compatible?start=${offset}&cpu=${forceCpu}` : source(m);
  function useCompatible(cpu = false) {
    offset = position;
    compatible = true;
    forceCpu = cpu;
    playerError = '';
  }
  let shuffled = $state(false),
    repeat = $state(false),
    playerError = $state('');
  let editing = $state<Media | null>(null),
    editName = $state(''),
    editGenre = $state('');
  let profileName = $state(''),
    profilePassword = $state(''),
    profileUpload = $state(false),
    blockedGenres = $state(''),
    blockedCollections = $state<string[]>([]),
    editingUser = $state('');
  let pin = $state(''),
    pairing = $state<{ id: string; pin: string; expires_in: number } | null>(null),
    pairingQr = $state('');
  let tvMode = $state(new URLSearchParams(location.search).has('tv'));
  let pairInterval: ReturnType<typeof setInterval> | undefined,
    toastTimer: ReturnType<typeof setTimeout>,
    pairInFlight = false;
  const activeCollection = $derived(collections.find((c) => c.id === selectedCollection));
  const filtered = $derived.by(() => {
    let result = items.filter(
      (m) =>
        (!['video', 'audio', 'photo'].includes(page) || m.kind === page) &&
        (!selectedCollection || m.collections.includes(selectedCollection)) &&
        `${m.name} ${m.genre}`.toLocaleLowerCase().includes(search.toLocaleLowerCase())
    );
    if (sort === 'name') result = [...result].sort((a, b) => a.name.localeCompare(b.name));
    return result;
  });
  const total = $derived(items.reduce((n, m) => n + m.size, 0));
  const title = $derived(
    activeCollection?.name ||
      {
        home: 'Sua coleção, em casa.',
        video: 'Hora de dar o play.',
        audio: 'O som da sua casa.',
        photo: 'Memórias que ficam.',
        collections: 'Tudo no seu lugar.'
      }[page] ||
      'Sua biblioteca'
  );
  const resume = $derived(items.filter((m) => m.kind === 'video' && m.seconds > 5));
  const initials = (value: string) =>
    value
      .split(/[\s._-]+/)
      .slice(0, 2)
      .map((w) => w[0])
      .join('')
      .toUpperCase();
  const date = (n: number) =>
    new Date(n * 1000).toLocaleDateString('pt-BR', {
      day: 'numeric',
      month: 'long',
      year: 'numeric'
    });
  function notify(text: string) {
    toast = text;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = ''), 5000);
  }
  async function action(fn: () => Promise<void>) {
    error = '';
    busy = true;
    try {
      await fn();
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }
  async function reload() {
    const result = await Promise.all([
      api<Media[]>('/media'),
      api<Collection[]>('/collections'),
      api<{ connected: boolean; items: number }>('/cloud/summary').catch(() => null)
    ]);
    items = result[0];
    collections = result[1];
    cloudSummary = result[2];
  }
  async function init() {
    try {
      const status = await api<{ setup: boolean; local: boolean; name: string; port: number }>(
        '/status'
      );
      setup = status.setup;
      local = status.local;
      serverName = status.name;
      serverPort = status.port;
      if (!setup) {
        try {
          user = await api<User>('/me');
          await reload();
        } catch {
          user = null;
        }
      }
      const incoming = new URLSearchParams(location.search).get('pair');
      if (incoming) {
        pin = incoming;
        if (user) modal = 'pair';
      }
    } catch (e) {
      error = (e as Error).message;
    } finally {
      ready = true;
    }
  }
  async function signIn() {
    await action(async () => {
      await api(setup ? '/setup' : '/login', 'POST', {
        name,
        password,
        setup_token: setupToken || null
      });
      password = '';
      setupToken = '';
      user = await api<User>('/me');
      setup = false;
      await reload();
      if (pin) modal = 'pair';
    });
  }
  async function signOut() {
    await action(async () => {
      await api('/logout', 'POST');
      current = null;
      user = null;
      items = [];
      profiles = [];
      settings = null;
      modal = '';
    });
  }
  function navigate(next: string, cid = '') {
    page = next;
    selectedCollection = cid;
    search = '';
  }
  async function upload(files: FileList | File[]) {
    if (!files.length || uploadProgress >= 0) return;
    error = '';
    modal = '';
    const list = Array.from(files);
    let done = 0;
    try {
      for (const file of list) {
        uploadName = file.name;
        uploadProgress = (done / list.length) * 100;
        await new Promise<void>((resolve, reject) => {
          const xhr = new XMLHttpRequest();
          xhr.open('POST', '/api/media');
          xhr.upload.onprogress = (e) => {
            if (e.lengthComputable)
              uploadProgress = ((done + e.loaded / e.total) / list.length) * 100;
          };
          xhr.onerror = () => reject(new Error('Conexão interrompida durante o envio'));
          xhr.onload = () => {
            if (xhr.status >= 200 && xhr.status < 300) resolve();
            else {
              try {
                reject(new Error(JSON.parse(xhr.responseText).error));
              } catch {
                reject(new Error('O envio falhou'));
              }
            }
          };
          const data = new FormData();
          data.append('file', file);
          xhr.send(data);
        });
        done++;
        await reload();
      }
      notify(`${done} arquivo${done > 1 ? 's' : ''} na sua biblioteca`);
    } catch (e) {
      error = (e as Error).message;
    } finally {
      uploadProgress = -1;
      if (uploadInput) uploadInput.value = '';
    }
  }
  function open(m: Media) {
    compatible = false;
    forceCpu = false;
    offset = 0;
    current = m;
    playerError = '';
    queue = filtered.filter((i) => i.kind === m.kind);
    position = m.seconds;
    duration = 0;
    lastSaved = m.seconds;
  }
  async function saveProgress(element = mediaElement) {
    const target = items.find((m) => m.id === element?.dataset.mediaId);
    if (target && target.kind !== 'photo' && element && Number.isFinite(element.currentTime)) {
      const seconds = element.currentTime + Number(element.dataset.offset || 0);
      target.seconds = seconds;
      try {
        await api(`/media/${target.id}/progress`, 'PUT', { seconds });
      } catch {
        /* Playback continues when the network is briefly unavailable. */
      }
    }
  }
  function timeUpdate() {
    if (!mediaElement) return;
    position = mediaElement.currentTime + (compatible ? offset : 0);
    duration = mediaElement.duration;
    if (Math.abs(position - lastSaved) >= 10) {
      lastSaved = position;
      void saveProgress();
    }
  }
  function loaded() {
    if (mediaElement && current) {
      if (!compatible && current.seconds > 0 && current.seconds < mediaElement.duration - 3)
        mediaElement.currentTime = current.seconds;
      duration = mediaElement.duration;
      void mediaElement.play().catch(() => (playing = false));
    }
  }
  async function closePlayer() {
    await saveProgress();
    current = null;
    playing = false;
  }
  function next(delta = 1) {
    if (!current || !queue.length) return;
    void saveProgress();
    const i = queue.findIndex((m) => m.id === current?.id);
    const n = shuffled
      ? Math.floor(Math.random() * queue.length)
      : (i + delta + queue.length) % queue.length;
    compatible = false;
    offset = 0;
    current = queue[n];
    playerError = '';
  }
  function ended() {
    if (!repeat && !shuffled && current?.id === queue.at(-1)?.id) {
      playing = false;
      void saveProgress();
      return;
    }
    if (repeat && mediaElement) {
      mediaElement.currentTime = 0;
      void mediaElement.play();
    } else next();
  }
  function togglePlay() {
    if (mediaElement) {
      if (mediaElement.paused)
        void mediaElement
          .play()
          .catch(() => (playerError = 'O aparelho não conseguiu reproduzir este formato.'));
      else mediaElement.pause();
    }
  }
  function edit(m: Media) {
    editing = m;
    editName = m.name;
    editGenre = m.genre;
    modal = 'edit';
  }
  async function openSettings() {
    await action(async () => {
      settings = await api<Settings>('/settings');
      modal = 'settings';
    });
  }
  async function openDrive() {
    await action(async () => {
      connectInfo = await api<Connect>('/connect');
      modal = 'drive';
    });
  }
  async function openProfiles() {
    await action(async () => {
      profiles = await api<User[]>('/users');
      modal = 'profiles';
    });
  }
  function profileForm(u?: User) {
    editingUser = u?.id || '';
    profileName = u?.name || '';
    profilePassword = '';
    profileUpload = u?.can_upload || false;
    blockedGenres = u?.blocked_genres.join(', ') || '';
    blockedCollections = u?.blocked_collections || [];
    modal = 'profile-form';
  }
  async function saveProfile() {
    await action(async () => {
      const data = {
        name: profileName,
        password: profilePassword,
        can_upload: profileUpload,
        blocked_genres: blockedGenres
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
        blocked_collections: blockedCollections
      };
      await api(
        editingUser ? `/users/${editingUser}` : '/users',
        editingUser ? 'PUT' : 'POST',
        data
      );
      profilePassword = '';
      profiles = await api<User[]>('/users');
      modal = 'profiles';
      notify('Perfil salvo');
    });
  }
  let pairLeft = $state(0),
    pairClock: ReturnType<typeof setInterval> | undefined;
  function stopPairing() {
    clearInterval(pairInterval);
    clearInterval(pairClock);
    pairing = null;
  }
  async function startPairing() {
    await action(async () => {
      pairing = await api('/pair', 'POST');
      if (!pairing) return;
      pairLeft = pairing.expires_in;
      clearInterval(pairClock);
      pairClock = setInterval(() => {
        pairLeft = Math.max(0, pairLeft - 1);
        // A fresh code appears by itself when the old one expires.
        if (pairLeft === 0 && pairing) void startPairing();
      }, 1000);
      pairingQr = await QRCode.toDataURL(`${location.origin}/?pair=${pairing.pin}`, {
        width: 230,
        margin: 2
      });
      clearInterval(pairInterval);
      pairInterval = setInterval(async () => {
        if (!pairing || pairInFlight) return;
        pairInFlight = true;
        try {
          const result = await api<{ approved: boolean }>(`/pair/${pairing.id}`, 'POST');
          if (result.approved) {
            stopPairing();
            user = await api<User>('/me');
            await reload();
          }
        } catch (e) {
          if (pairLeft <= 2) return; // expired: a new code is on its way
          stopPairing();
          error = (e as Error).message;
        } finally {
          pairInFlight = false;
        }
      }, 2500);
    });
  }
  let dialogOpen = false;
  let previousFocus: HTMLElement | null = null;
  $effect(() => {
    const open = !!modal || !!(current && current.kind !== 'audio');
    if (open && !dialogOpen) {
      previousFocus = document.activeElement as HTMLElement;
      void tick().then(() =>
        document
          .querySelector<HTMLElement>('[role="dialog"] input, [role="dialog"] button')
          ?.focus()
      );
    } else if (!open && dialogOpen) previousFocus?.focus();
    dialogOpen = open;
  });
  function keydown(e: KeyboardEvent) {
    if (e.key === 'Tab') {
      const dialog = document.querySelector('[role="dialog"]');
      if (dialog) {
        const focusable = [
          ...dialog.querySelectorAll<HTMLElement>(
            'button:not(:disabled),input,select,video[controls]'
          )
        ].filter((n) => n.getBoundingClientRect().width > 0);
        const index = focusable.indexOf(document.activeElement as HTMLElement);
        if (e.shiftKey && index <= 0) {
          e.preventDefault();
          focusable.at(-1)?.focus();
        } else if (!e.shiftKey && (index < 0 || index === focusable.length - 1)) {
          e.preventDefault();
          focusable[0]?.focus();
        }
      }
    }

    if (e.key === 'Escape' || e.key === 'BrowserBack') {
      if (modal) modal = '';
      else if (current) void closePlayer();
      return;
    }
    if (
      !tvMode ||
      document.querySelector('.pv') ||
      !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key) ||
      (e.target instanceof HTMLElement &&
        ['INPUT', 'TEXTAREA', 'SELECT', 'VIDEO', 'AUDIO'].includes(e.target.tagName))
    )
      return;
    const scope = document.querySelector('[role="dialog"]') || document;
    const nodes = [
      ...scope.querySelectorAll<HTMLElement>('button:not(:disabled),a[href],input,select')
    ].filter((n) => n.getBoundingClientRect().width > 0);
    const active = document.activeElement as HTMLElement;
    if (!nodes.includes(active)) {
      nodes[0]?.focus();
      return;
    }
    const rect = active.getBoundingClientRect(),
      x = rect.x + rect.width / 2,
      y = rect.y + rect.height / 2;
    const candidates = nodes
      .filter((n) => n !== active)
      .map((n) => {
        const r = n.getBoundingClientRect();
        const dx = r.x + r.width / 2 - x,
          dy = r.y + r.height / 2 - y;
        const horizontal = e.key === 'ArrowLeft' || e.key === 'ArrowRight';
        const along = horizontal ? dx : dy,
          across = horizontal ? dy : dx;
        const sign = e.key === 'ArrowLeft' || e.key === 'ArrowUp' ? -1 : 1;
        return { n, valid: along * sign > 5, distance: Math.abs(along) + Math.abs(across) * 3 };
      })
      .filter((n) => n.valid)
      .sort((a, b) => a.distance - b.distance);
    if (candidates[0]) {
      e.preventDefault();
      candidates[0].n.focus();
      candidates[0].n.scrollIntoView({ block: 'nearest' });
    }
  }
  onMount(() => {
    void init();
    return () => {
      clearInterval(pairInterval);
      clearTimeout(toastTimer);
    };
  });
</script>

<svelte:window onkeydown={keydown} />
<input
  class="hidden"
  bind:this={uploadInput}
  type="file"
  multiple
  accept="image/jpeg,image/png,image/webp,image/gif,image/avif,audio/*,video/*,.mkv,.flac"
  onchange={(e) => e.currentTarget.files && upload(e.currentTarget.files)}
/>

{#if !ready}
  <main class="loading">
    <LoaderCircle class="spin" size={30} />
    <p>Abrindo sua casa…</p>
  </main>
{:else if !user}
  <main class="auth-layout">
    <section class="auth-story">
      <a class="brand" href="/"
        ><span class="brand-mark"><Play fill="currentColor" size={19} /></span>sonam<span
          class="brand-dot">.</span
        ></a
      >
      <div>
        <span class="eyebrow">SUA MÍDIA. SEU ESPAÇO.</span>
        <h1>As melhores {' '}<br />coleções têm {' '}<br /><em>história.</em></h1>
        <p>Seus filmes, músicas e memórias.<br />Todos juntos. Todos em casa.</p>
        <div class="story-icons"><Film /><Music2 /><Images /></div>
      </div>
      <span class="small muted"><CloudOff size={14} /> Sem nuvem. Sem assinatura. Só você.</span>
    </section>
    <section class="auth-form">
      {#if pairing}
        <span class="eyebrow">ENTRAR SEM DIGITAR SENHA</span>
        <h2>Autorize esta tela<br />pelo celular.</h2>
        <ol class="pair-steps">
          <li>
            <span>1</span>
            <p>
              Pegue um celular ou computador que <strong>já entrou</strong> no Sonam.
            </p>
          </li>
          <li>
            <span>2</span>
            <p>
              Aponte a câmera para o QR <em>ou</em> abra <strong>seu perfil › Parear tela</strong> e digite
              o código.
            </p>
          </li>
          <li>
            <span>3</span>
            <p>Pronto: esta tela entra sozinha, com o perfil de quem autorizou.</p>
          </li>
        </ol>
        <div class="pair-card">
          <img class="qr" src={pairingQr} alt="QR code que abre a autorização desta tela" />
          <div>
            <span class="pair-label">Código</span>
            <strong class="pair-pin" aria-label={`Código ${pairing.pin.split('').join(' ')}`}
              >{pairing.pin.slice(0, 3)}<i></i>{pairing.pin.slice(3)}</strong
            >
            <span class="pair-wait"
              ><LoaderCircle size={14} class="spin" /> Aguardando autorização · novo código em {clock(
                pairLeft
              )}</span
            >
          </div>
        </div>
        <button class="text-button" onclick={stopPairing}
          ><ArrowLeft size={17} />Voltar e entrar com senha</button
        >
      {:else}
        <span class="eyebrow">{setup ? 'BEM-VINDO AO SONAM' : 'BOM TER VOCÊ EM CASA'}</span>
        <h2>{setup ? 'Sua casa começa aqui.' : 'Entre na sua coleção.'}</h2>
        <p>
          {setup
            ? 'Crie a conta de administrador: é ela que convida a família, cria perfis e cuida das configurações.'
            : 'Entre com o nome e a senha do seu perfil.'}
        </p>
        {#if setup && !local}
          <div class="setup-key-box" role="note">
            <ShieldCheck size={20} />
            <div>
              <strong>Você está criando a conta por outro aparelho.</strong>
              <p>
                Por segurança, isso pede a <em>chave de primeiro acesso</em>, para que um vizinho na
                mesma rede não tome o servidor antes de você. Ela aparece na janela ou no terminal
                onde o Sonam foi aberto (no Docker: <code>docker compose logs</code>).
              </p>
              <p>
                <strong>Mais fácil:</strong> crie a conta no próprio servidor. Se o servidor é um
                celular, abra o app Sonam nele; no computador, abra
                <code>http://localhost:{serverPort}</code>. Aí a chave não é pedida.
              </p>
            </div>
          </div>
        {/if}
        <form
          onsubmit={(e) => {
            e.preventDefault();
            void signIn();
          }}
        >
          <label
            >Seu nome<input
              required
              maxlength="80"
              autocomplete="username"
              bind:value={name}
              placeholder="Como podemos chamar você?"
            /></label
          ><label
            >Senha<input
              required
              type="password"
              minlength={setup ? 10 : 1}
              maxlength="256"
              autocomplete={setup ? 'new-password' : 'current-password'}
              bind:value={password}
              placeholder={setup ? 'Pelo menos 10 caracteres' : 'Sua senha'}
            /></label
          >
          {#if setup && !local}<label
              >Chave de primeiro acesso<input
                required
                autocomplete="off"
                autocapitalize="off"
                spellcheck="false"
                bind:value={setupToken}
                placeholder="Cole aqui a chave mostrada pelo servidor"
              /><small>Só é pedida uma vez, na criação do administrador.</small></label
            >{/if}
          {#if error}<div class="error" role="alert">{error}</div>{/if}<button
            class="primary"
            disabled={busy}
            >{busy ? 'Só um instante…' : setup ? 'Criar minha casa' : 'Entrar'}<ArrowUpRight
              size={18}
            /></button
          >
        </form>
        {#if !setup}
          <div class="pair-login-box">
            <span class="or">ou</span>
            <button class="secondary pair-login" disabled={busy} onclick={startPairing}
              ><Tv size={18} />Entrar com um código (TV, sem digitar senha)</button
            >
            <p>Mostra um código nesta tela. Você autoriza pelo celular onde já está conectado.</p>
          </div>
        {/if}
        <p class="auth-foot"><ShieldCheck size={15} /> Local por natureza. Privado por escolha.</p>
      {/if}
    </section>
  </main>
{:else}
  <div class:tv={tvMode} class:with-player={current?.kind === 'audio'} class="app-shell">
    <aside class="sidebar">
      <a
        href="/"
        onclick={(e) => {
          e.preventDefault();
          navigate('home');
        }}
        class="brand"
        ><span class="brand-mark"><Play fill="currentColor" size={17} /></span>sonam<span
          class="brand-dot">.</span
        ></a
      >
      <div class="home-label"><span class="status-dot"></span> SUA CASA ESTÁ ONLINE</div>
      <nav aria-label="Biblioteca">
        <span class="nav-label">BIBLIOTECA</span>
        {#each [{ id: 'home', label: 'Visão geral', icon: House }, { id: 'video', label: 'Filmes e vídeos', icon: Film }, { id: 'audio', label: 'Músicas', icon: Music2 }, { id: 'photo', label: 'Fotos', icon: Images }, { id: 'collections', label: 'Coleções', icon: Library }, ...(user.admin || user.can_upload ? [{ id: 'files', label: 'Arquivos', icon: FolderOpen }] : []), ...(user.admin || cloudSummary?.items ? [{ id: 'cloud', label: 'Google Drive', icon: Cloud }] : [])] as link}
          <button
            class:active={page === link.id && !selectedCollection}
            onclick={() => navigate(link.id)}
            ><link.icon size={19} />{link.label}{#if link.id === 'photo'}<span class="nav-count"
                >{items.filter((m) => m.kind === 'photo').length}</span
              >{/if}</button
          >
        {/each}
      </nav>
      <div class="collection-label">
        <span class="nav-label">DO SEU JEITO</span>{#if user.admin}<button
            class="icon-button"
            aria-label="Criar coleção"
            onclick={() => (modal = 'collection')}><Plus size={16} /></button
          >{/if}
      </div>
      <div class="collection-nav">
        {#each collections as c}<button
            class:active={selectedCollection === c.id}
            onclick={() => navigate(c.kind, c.id)}
            ><span
              class="collection-dot"
              class:audio={c.kind === 'audio'}
              class:photo={c.kind === 'photo'}
            ></span><span>{c.name}</span></button
          >{:else}<p class="muted small collection-hint">
            Um lugar para cada<br />uma das suas histórias.
          </p>{/each}
      </div>
      <div class="sidebar-bottom">
        <div class="storage-label"><HardDrive size={16} /><span>Armazenamento local</span></div>
        <div class="storage-bar" aria-hidden="true"></div>
        <span class="small muted">{size(total)} na sua biblioteca</span>
        <button class="profile-button" onclick={() => (modal = 'account')}
          ><span class="avatar">{initials(user.name)}</span><span
            ><strong>{user.name}</strong><small>{user.admin ? 'Administrador' : 'Meu perfil'}</small
            ></span
          ><MoreHorizontal size={18} /></button
        >
      </div>
    </aside>
    <div class="workspace">
      <header class="topbar">
        <div class="breadcrumb">
          Minha casa <ChevronRight size={13} /><strong
            >{activeCollection?.name ||
              {
                home: 'Visão geral',
                collections: 'Coleções',
                files: 'Arquivos',
                cloud: 'Google Drive'
              }[page] ||
              labels[page as keyof typeof labels]}</strong
          >
        </div>
        <div class="top-actions">
          <span
            class="local-badge"
            class:cloud-on={cloudSummary?.connected}
            title={cloudSummary?.connected
              ? 'Arquivos no servidor, com cópia no Google Drive'
              : undefined}
            ><span class="status-dot"></span>{cloudSummary?.connected
              ? 'Local + Drive'
              : '100% local'}</span
          ><button class="icon-button" aria-label="Parear tela" onclick={() => (modal = 'pair')}
            ><Monitor size={19} /></button
          >{#if user.admin}<button
              class="icon-button"
              aria-label="Configurações"
              onclick={openSettings}><SettingsIcon size={19} /></button
            >{/if}<button
            class="avatar tiny"
            aria-label="Meu perfil"
            onclick={() => (modal = 'account')}>{initials(user.name)}</button
          >
        </div>
      </header>
      <main class="content">
        {#if page === 'cloud'}
          <div class="page-heading">
            <div>
              <span class="eyebrow">A CÓPIA FORA DE CASA</span>
              <h1>Google Drive</h1>
              <p>
                O que o Sonam guardou na sua conta Google, com as mesmas telas da biblioteca.
                Inclui o que foi liberado deste servidor.
              </p>
            </div>
          </div>
          {#key cloudKey}<CloudBrowser
              {user}
              {notify}
              {current}
              onmanage={() => (modal = 'cloud')}
              onchanged={() => void reload()}
              onopen={(m, q) => {
                open(m);
                queue = q;
              }}
            />{/key}
        {:else if page === 'files'}
          <div class="page-heading">
            <div>
              <span class="eyebrow">O DISCO DA SUA CASA</span>
              <h1>Arquivos</h1>
              <p>
                Tudo o que está guardado no servidor, em pastas. É o mesmo conteúdo da unidade de
                rede no computador.
              </p>
            </div>
            <button class="secondary" onclick={openDrive}
              ><HardDrive size={17} />Abrir no computador</button
            >
          </div>
          <Files
            {notify}
            onchanged={() => void reload()}
            onopen={(id, siblings) => {
              const m = items.find((i) => i.id === id);
              if (!m) {
                notify('Ainda adicionando à biblioteca. Tente de novo em instantes.');
                void reload();
                return;
              }
              open(m);
              queue = siblings
                .map((s) => items.find((i) => i.id === s))
                .filter((x): x is Media => !!x);
            }}
          />
        {:else}
          <div class="page-heading">
            <div>
              <span class="eyebrow"
                >{page === 'home'
                  ? 'SEU PEQUENO UNIVERSO'
                  : activeCollection
                    ? 'FEITO DO SEU JEITO'
                    : 'BIBLIOTECA PESSOAL'}</span
              >
              <h1>{title}</h1>
              <p>
                {page === 'home'
                  ? 'Um lugar para tudo o que vale guardar.'
                  : page === 'photo'
                    ? 'Cada foto, um pedacinho da sua história.'
                    : page === 'audio'
                      ? 'Seus favoritos. No seu ritmo.'
                      : page === 'video'
                        ? 'A próxima sessão já está em casa.'
                        : 'Álbuns, playlists e boas combinações.'}
              </p>
            </div>
            {#if user.can_upload || user.admin}<button
                class="primary upload-button"
                disabled={uploadProgress >= 0}
                onclick={() => uploadInput.click()}><Plus size={18} />Adicionar arquivos</button
              >{/if}
          </div>
          {#if error}<div class="error dismissable" role="alert">
              {error}<button
                class="icon-button"
                aria-label="Fechar erro"
                onclick={() => (error = '')}><X size={16} /></button
              >
            </div>{/if}
          {#if uploadProgress >= 0}<div class="upload-progress">
              <LoaderCircle class="spin" size={18} />
              <div>
                <strong>Guardando {uploadName}</strong><progress max="100" value={uploadProgress}
                ></progress>
              </div>
              <span>{Math.round(uploadProgress)}%</span>
            </div>{/if}
          {#if page === 'home' && !search && !selectedCollection}
            <section class="hero">
              <div class="hero-copy">
                <span class="hero-tag"
                  ><span class="status-dot"></span> A MELHOR CONEXÃO É A DE CASA</span
                >
                <h2>Sua vida tem<br />uma ótima trilha.</h2>
                <p>
                  Dê play no que é seu. Sem algoritmos,<br />sem interrupções, sem sair de casa.
                </p>
                <button class="hero-button" onclick={() => navigate('audio')}
                  ><Play size={16} fill="currentColor" />Explorar minhas músicas<ArrowUpRight
                    size={17}
                  /></button
                >
              </div>
              <div class="record-art" aria-hidden="true">
                <div class="orbit orbit-one"></div>
                <div class="orbit orbit-two"></div>
                <div class="vinyl">
                  <div class="vinyl-label">
                    <span>h.</span><small>YOUR OWN<br />ROTATION</small>
                  </div>
                </div>
                <div class="record-caption">
                  <Music2 size={16} /><span>O seu som.<br /><strong>Do seu jeito.</strong></span>
                </div>
                <span class="art-spark">✳</span>
              </div>
            </section>
            <section class="type-grid" aria-label="Tipos de mídia">
              {#each [{ kind: 'video', icon: Film, label: 'Filmes e vídeos', sub: 'Sua próxima sessão', color: 'peach' }, { kind: 'audio', icon: Music2, label: 'Músicas', sub: 'Aperte o play', color: 'lavender' }, { kind: 'photo', icon: Images, label: 'Fotos', sub: 'Memórias por perto', color: 'sage' }] as t}<button
                  class="type-card"
                  onclick={() => navigate(t.kind)}
                  ><span class="type-icon {t.color}"><t.icon size={22} /></span><span
                    ><strong>{t.label}</strong><small
                      >{items.filter((m) => m.kind === t.kind).length} arquivos · {t.sub}</small
                    ></span
                  ><ArrowUpRight size={18} /></button
                >{/each}
            </section>
            {#if resume.length}<section class="resume-section">
                <div class="section-title">
                  <h2>De onde você parou</h2>
                  <span>Seu tempo, seu ritmo</span>
                </div>
                <div class="resume-grid">
                  {#each resume.slice(0, 3) as m}<button class="resume-card" onclick={() => open(m)}
                      ><span class="resume-play"><Play size={19} fill="currentColor" /></span><span
                        ><strong>{m.name}</strong><small>Continuar em {clock(m.seconds)}</small
                        ></span
                      ><ChevronRight size={18} /></button
                    >{/each}
                </div>
              </section>{/if}
          {/if}
          <section class="library-section">
            <div class="section-title library-title">
              <div>
                <h2>
                  {page === 'collections'
                    ? 'Suas coleções'
                    : search
                      ? 'Resultados da pesquisa'
                      : page === 'home'
                        ? 'Adicionados recentemente'
                        : activeCollection
                          ? 'Nesta coleção'
                          : 'Sua biblioteca'}<span class="count-pill"
                    >{page === 'collections' ? collections.length : filtered.length}</span
                  >
                </h2>
                {#if page === 'home'}<p>Novas histórias para a sua coleção.</p>{/if}
              </div>
              <div class="library-tools">
                <label class="search-box"
                  ><Search size={16} /><input
                    aria-label="Pesquisar na biblioteca"
                    placeholder="Buscar na sua coleção"
                    bind:value={search}
                  />{#if search}<button
                      class="icon-button"
                      aria-label="Limpar pesquisa"
                      onclick={() => (search = '')}><X size={14} /></button
                    >{/if}</label
                >{#if page !== 'collections'}<label class="sort-box"
                    ><ArrowDownUp size={15} /><select
                      aria-label="Ordenar arquivos"
                      bind:value={sort}
                      ><option value="recent">Mais recentes</option><option value="name"
                        >Nome A–Z</option
                      ></select
                    ></label
                  >{/if}
              </div>
            </div>
            {#if page === 'collections'}
              <div class="collections-grid">
                {#each collections.filter((c) => c.name
                    .toLowerCase()
                    .includes(search.toLowerCase())) as c}<button
                    class="collection-card {c.kind}"
                    onclick={() => navigate(c.kind, c.id)}
                    ><span class="collection-art"
                      >{#if c.kind === 'photo'}<Images
                          size={40}
                        />{:else if c.kind === 'audio'}<Music2 size={40} />{:else}<Film
                          size={40}
                        />{/if}</span
                    ><strong>{c.name}</strong><small
                      >{items.filter((m) => m.collections.includes(c.id)).length} arquivos · {c.kind ===
                      'photo'
                        ? 'Álbum'
                        : 'Playlist'}</small
                    ><ArrowUpRight size={18} /></button
                  >{/each}{#if user.admin}<button
                    class="new-collection"
                    onclick={() => (modal = 'collection')}
                    ><FolderPlus size={27} /><strong>Criar uma coleção</strong><small
                      >Organize do seu jeito</small
                    ></button
                  >{/if}
              </div>
            {:else if filtered.length}
              {#if page === 'audio'}<div class="track-list">
                  <div class="track-header">
                    <span>#</span><span>TÍTULO</span><span>GÊNERO</span><span>ARQUIVO</span>
                  </div>
                  {#each filtered as m, i}<div
                      class="track"
                      class:now-playing={current?.id === m.id}
                    >
                      <button
                        class="track-number"
                        aria-label={'Reproduzir ' + m.name}
                        onclick={() => open(m)}
                        >{#if current?.id === m.id}<Volume2 size={16} />{:else}{String(
                            i + 1
                          ).padStart(2, '0')}{/if}</button
                      ><button class="track-title" onclick={() => open(m)}
                        ><span class="track-art"><Music2 size={21} /></span><span
                          ><strong>{m.name}</strong><small>{date(m.created)}</small></span
                        ></button
                      ><span class="muted">{m.genre || 'Sem gênero'}</span><span class="muted"
                        >{#if m.backup === 'ok'}<span
                            class="backup-dot"
                            title="Com cópia no Google Drive"
                            ><CloudCheck size={14} /><span class="sr-only"
                              >Com cópia no Google Drive</span
                            ></span
                          >{/if}{size(m.size)}{#if user.admin}<button
                            class="icon-button"
                            aria-label={'Editar ' + m.name}
                            onclick={() => edit(m)}><MoreHorizontal size={18} /></button
                          >{/if}</span
                      >
                    </div>{/each}
                </div>
              {:else}<div class="media-grid" class:photo-grid={page === 'photo'}>
                  {#each page === 'home' ? filtered.slice(0, 12) : filtered as m, i}<article
                      class="media-card"
                    >
                      <button
                        class="media-cover cover-{i % 6}"
                        class:photo={m.kind === 'photo'}
                        onclick={() => open(m)}
                        aria-label={'Abrir ' + m.name}
                        >{#if m.kind === 'photo'}<img
                            src={`/api/media/${m.id}/thumbnail`}
                            alt={m.name}
                            loading="lazy"
                          />{:else}<span class="cover-number">{String(i + 1).padStart(2, '0')}</span
                          ><span class="cover-monogram">{initials(m.name)}</span><span
                            class="cover-kind"
                            >{m.kind === 'video' ? 'HOME CINEMA' : 'HOME SESSIONS'}</span
                          ><span class="cover-play"><Play size={23} fill="currentColor" /></span
                          >{/if}<span class="media-format"
                          >{m.mime.split('/')[1].toUpperCase()}</span
                        ></button
                      >
                      <div class="media-info">
                        <div>
                          <button onclick={() => open(m)}>{m.name}</button>
                          <p>
                            {m.genre || labels[m.kind]}<span>·</span>{size(
                              m.size
                            )}{#if m.backup === 'ok'}<span>·</span><span
                                class="backup-dot"
                                title="Com cópia no Google Drive"
                                ><CloudCheck size={12} /><span class="sr-only"
                                  >Com cópia no Google Drive</span
                                ></span
                              >{/if}
                          </p>
                        </div>
                        {#if user.admin}<button
                            class="icon-button"
                            aria-label={'Organizar ' + m.name}
                            onclick={() => edit(m)}><MoreHorizontal size={18} /></button
                          >{/if}
                      </div>
                    </article>{/each}
                </div>{/if}
            {:else}<div class="empty-state">
                <span class="empty-icon"
                  >{#if search}<Search size={30} />{:else if page === 'photo'}<Images
                      size={30}
                    />{:else if page === 'audio'}<Music2 size={30} />{:else}<Library
                      size={30}
                    />{/if}</span
                >
                <h3>
                  {search
                    ? 'Ainda não encontramos essa história.'
                    : 'Toda coleção começa com um favorito.'}
                </h3>
                <p>
                  {search
                    ? 'Tente outro nome ou gênero.'
                    : 'Adicione fotos, músicas ou vídeos. Eles ficam guardados aqui, com você.'}
                </p>
                {#if !search && (user.can_upload || user.admin)}<button
                    class="secondary"
                    onclick={() => uploadInput.click()}
                    ><Upload size={17} />Escolher meus arquivos</button
                  >{/if}
              </div>{/if}
          </section>
        {/if}
        <footer class="page-footer">
          <span><span class="brand-mini">h.</span>Feito para guardar o que importa.</span><span
            ><ShieldCheck size={14} /> Seus arquivos ficam em casa.</span
          >
        </footer>
      </main>
    </div>
  </div>
{/if}

{#if user && modal}
  <div class="modal-backdrop">
    <div
      class="modal"
      class:wide={modal === 'settings' ||
        modal === 'profiles' ||
        modal === 'drive' ||
        modal === 'cloud'}
      role="dialog"
      aria-modal="true"
      aria-label="Opções do Sonam"
      tabindex="-1"
    >
      <button class="modal-close icon-button" aria-label="Fechar" onclick={() => (modal = '')}
        ><X size={20} /></button
      >
      {#if modal === 'cloud' && user.admin}<CloudBackup
          {notify}
          onchanged={() => {
            void reload();
            cloudKey++;
          }}
          onbrowse={() => {
            modal = '';
            navigate('cloud');
            cloudKey++;
          }}
        />
      {:else if modal === 'collection'}<span class="modal-icon"><FolderPlus /></span><span
          class="eyebrow">FEITO DO SEU JEITO</span
        >
        <h2>Um lugar para seus favoritos.</h2>
        <p>Crie uma playlist de vídeos, músicas ou um álbum de fotos.</p>
        <form
          onsubmit={(e) => {
            e.preventDefault();
            void action(async () => {
              await api('/collections', 'POST', { name: collectionName, kind: collectionKind });
              collectionName = '';
              await reload();
              modal = '';
              notify('Coleção criada');
            });
          }}
        >
          <label
            >Nome da coleção<input
              required
              maxlength="100"
              bind:value={collectionName}
              placeholder="Domingos em família"
            /></label
          ><label
            >O que vamos guardar?<select bind:value={collectionKind}
              ><option value="video">Filmes e vídeos</option><option value="audio">Músicas</option
              ><option value="photo">Fotos</option></select
            ></label
          ><button class="primary" disabled={busy}>Criar coleção<Plus size={17} /></button>
        </form>
      {:else if modal === 'pair'}<span class="modal-icon"><Monitor /></span><span class="eyebrow"
          >MAIS TELAS, A MESMA CASA</span
        >
        <h2>Autorizar uma TV ou outro aparelho.</h2>
        <ol class="pair-steps compact">
          <li>
            <span>1</span>
            <p>
              Na outra tela, abra <strong
                >{serverName ? `http://${serverName}.local:${serverPort}` : location.origin}</strong
              >
              e toque em <strong>Entrar com um código</strong>.
            </p>
          </li>
          <li>
            <span>2</span>
            <p>Digite abaixo os 6 números que aparecerem lá (ou leia o QR com a câmera).</p>
          </li>
        </ol>
        <p class="pair-warning">
          A tela vai entrar como <strong>{user.name}</strong>, com as mesmas permissões e restrições
          deste perfil, por até 30 dias.
        </p>
        <form
          onsubmit={(e) => {
            e.preventDefault();
            void action(async () => {
              await api('/pair/approve', 'POST', { pin });
              pin = '';
              modal = '';
              history.replaceState({}, '', location.pathname);
              notify('Tela autorizada. Já pode dar o play.');
            });
          }}
        >
          <label
            >Código da outra tela<input
              class="pin-input"
              required
              pattern="[0-9]{6}"
              maxlength="6"
              inputmode="numeric"
              bind:value={pin}
              placeholder="000000"
            /></label
          ><button class="primary" disabled={busy}>Autorizar esta tela<Check size={18} /></button>
        </form>
      {:else if modal === 'account'}<span class="avatar large">{initials(user.name)}</span>
        <h2>Olá, {user.name}.</h2>
        <p>Sua coleção acompanha o seu perfil.</p>
        <div class="account-actions">
          <button class="secondary" onclick={() => (modal = 'pair')}
            ><Monitor size={18} />Parear uma tela</button
          ><button
            class="secondary"
            onclick={() => {
              tvMode = !tvMode;
              modal = '';
            }}><Tv size={18} />{tvMode ? 'Sair do modo TV' : 'Ativar modo TV'}</button
          >{#if user.admin || user.can_upload}<button class="secondary" onclick={openDrive}
              ><HardDrive size={18} />Unidade de rede</button
            >{/if}{#if user.admin}<button class="secondary" onclick={openProfiles}
              ><Users size={18} />Gerenciar perfis</button
            ><button class="secondary" onclick={() => (modal = 'cloud')}
              ><Cloud size={18} />Backup no Google Drive</button
            ><button class="secondary" onclick={openSettings}
              ><SettingsIcon size={18} />Configurações da casa</button
            >{/if}<button class="text-button danger" onclick={signOut}
            ><LogOut size={18} />Sair do meu perfil</button
          >
        </div>
      {:else if modal === 'settings' && settings}<span class="eyebrow">SUA CASA, SUAS REGRAS</span>
        <h2>Conexões e reprodução.</h2>
        <Addresses bind:settings {notify} run={action} />
        <p class="tv-hint">
          Na TV com navegador, adicione <code>/?tv=1</code> ao endereço para navegar com as setas do controle.
        </p>
        <button class="secondary settings-drive" onclick={openDrive}
          ><HardDrive size={17} />Usar como unidade de rede no computador<ChevronRight
            size={17}
          /></button
        >
        <div class="setting-row">
          <div>
            <strong>Preferir aceleração de GPU</strong>
            <p>
              Usa GPU na conversão quando o teste de hardware funciona. Desative para usar CPU.
              Reprodução direta não usa GPU do servidor.
            </p>
          </div>
          <input
            type="checkbox"
            role="switch"
            aria-label="Preferir aceleração de GPU"
            checked={settings.gpu}
            onchange={(e) => {
              const gpu = e.currentTarget.checked;
              void action(async () => {
                await api('/settings', 'PUT', { gpu });
                if (settings) settings.gpu = gpu;
                notify('Preferência salva');
              });
            }}
          />
        </div>
        <div class="info-box">
          <ShieldCheck size={20} />
          <p>
            <strong>Reprodução direta por padrão.</strong> O arquivo original vai para o player, sem
            reconversão e sem exigir GPU do servidor. Para codecs incompatíveis, ative o modo
            compatível no player. FFmpeg no servidor: {settings.transcoding_available
              ? 'disponível'
              : 'não instalado'}. GPU detectada: {settings.encoder || 'nenhuma; usa CPU'}.
          </p>
        </div>
        <div class="setting-row">
          <div>
            <strong>Armazenamento</strong>
            <p>{settings.data_path}</p>
          </div>
          <span>{size(settings.used_bytes)}</span>
        </div>
        <button class="secondary" onclick={() => (modal = 'cloud')}
          ><Cloud size={17} />Backup no Google Drive<ChevronRight size={17} /></button
        >
        <button class="secondary" onclick={openProfiles}
          ><Users size={17} />Perfis e permissões<ChevronRight size={17} /></button
        >
      {:else if modal === 'drive' && connectInfo}<span class="modal-icon"><HardDrive /></span><span
          class="eyebrow">UM DISCO DA CASA EM CADA COMPUTADOR</span
        >
        <h2>Unidade de rede.</h2>
        <p>
          Abra o Sonam como um disco no Explorador de Arquivos, Finder ou gerenciador do Linux.
          Arraste arquivos para dentro e eles ficam guardados no servidor.
        </p>
        <NetworkDrive info={connectInfo} admin={user.admin} {notify} run={action} />
      {:else if modal === 'profiles'}<span class="eyebrow">CADA UM NO SEU ESPAÇO</span>
        <h2>Quem está em casa?</h2>
        <p>As restrições valem na biblioteca, pesquisa e no acesso direto aos arquivos.</p>
        <div class="profiles">
          {#each profiles as u}<div class="profile-row">
              <span class="avatar">{initials(u.name)}</span><span
                ><strong>{u.name}</strong><small
                  >{u.admin
                    ? 'Administrador · acesso completo'
                    : `${u.can_upload ? 'Pode enviar arquivos' : 'Somente reprodução'} · ${u.blocked_genres.length + u.blocked_collections.length} restrições`}</small
                ></span
              >{#if !u.admin}<button
                  class="icon-button"
                  aria-label={'Editar perfil de ' + u.name}
                  onclick={() => profileForm(u)}><Pencil size={17} /></button
                >{/if}
            </div>{/each}
        </div>
        <button class="primary" onclick={() => profileForm()}
          ><Plus size={17} />Adicionar perfil</button
        >
      {:else if modal === 'profile-form'}<span class="eyebrow">PERFIL E PERMISSÕES</span>
        <h2>{editingUser ? `Perfil de ${profileName}` : 'Mais alguém em casa.'}</h2>
        <form
          onsubmit={(e) => {
            e.preventDefault();
            void saveProfile();
          }}
        >
          {#if !editingUser}<label
              >Nome<input required maxlength="80" bind:value={profileName} /></label
            ><label
              >Senha<input
                required
                type="password"
                minlength="10"
                maxlength="256"
                autocomplete="new-password"
                bind:value={profilePassword}
              /></label
            >{/if}<label class="checkbox-label"
            ><input type="checkbox" bind:checked={profileUpload} />Permitir envio de arquivos</label
          ><label
            >Gêneros bloqueados<input
              bind:value={blockedGenres}
              placeholder="Terror, adulto"
            /><small>Separe por vírgulas. O bloqueio usa o gênero cadastrado no arquivo.</small
            ></label
          >
          <fieldset>
            <legend>Coleções bloqueadas</legend>{#each collections as c}<label
                class="checkbox-label"
                ><input
                  type="checkbox"
                  value={c.id}
                  bind:group={blockedCollections}
                />{c.name}</label
              >{:else}<p class="muted small">Crie coleções para configurar bloqueios.</p>{/each}
          </fieldset>
          <p class="small muted">
            Bloquear uma coleção também bloqueia seus arquivos quando aparecem em outras coleções.
            Gêneros são classificados pelo administrador; não há moderação automática.
          </p>
          <button class="primary" disabled={busy}>Salvar perfil<Check size={17} /></button>
        </form>
      {:else if modal === 'edit' && editing}<span class="eyebrow">UM POUCO DE ORGANIZAÇÃO</span>
        <h2>Detalhes do arquivo.</h2>
        <form
          onsubmit={(e) => {
            e.preventDefault();
            void action(async () => {
              await api(`/media/${editing!.id}`, 'PUT', { name: editName, genre: editGenre });
              await reload();
              modal = '';
              notify('Arquivo atualizado');
            });
          }}
        >
          <label>Nome<input required maxlength="240" bind:value={editName} /></label><label
            >Gênero ou categoria<input
              maxlength="80"
              bind:value={editGenre}
              placeholder="Viagens, jazz, documentário…"
            /></label
          ><button class="primary" disabled={busy}>Salvar detalhes<Check size={17} /></button>
        </form>
        {#if editing.path}<p class="drive-path">
            <HardDrive size={14} />Na unidade de rede: <code>{editing.path}</code>
          </p>{/if}
        {#if cloudSummary?.connected || editing.backup}
          <div class="backup-state" class:ok={editing.backup === 'ok'}>
            {#if editing.backup === 'ok'}<CloudCheck size={18} />{:else}<CloudUpload
                size={18}
              />{/if}
            <div>
              <strong
                >{editing.backup === 'ok'
                  ? 'Cópia conferida no Google Drive'
                  : editing.backup === 'outdated'
                    ? 'Mudou desde a última cópia no Drive'
                    : 'Ainda sem cópia no Google Drive'}</strong
              >
              <small
                >{editing.backup === 'ok'
                  ? `Dá para liberar ${size(editing.size)} deste servidor e continuar vendo pela página Google Drive.`
                  : 'Use “Sincronizar agora” no Backup no Google Drive.'}</small
              >
            </div>
            {#if editing.backup === 'ok'}<button
                class="secondary"
                disabled={busy}
                onclick={() => {
                  void action(async () => {
                    await api('/cloud/free', 'POST', { media: [editing!.id] });
                    if (current?.id === editing!.id) current = null;
                    await reload();
                    cloudKey++;
                    editing = null;
                    modal = '';
                    notify('Agora está só no Google Drive. Abra pela página Google Drive.');
                  });
                }}>Liberar daqui</button
              >{/if}
          </div>
        {/if}
        <h3 class="subheading">Adicionar a uma coleção</h3>
        <div class="edit-collections">
          {#each collections.filter((c) => c.kind === editing?.kind) as c}<button
              class="secondary"
              onclick={() => {
                void action(async () => {
                  const added = editing!.collections.includes(c.id);
                  await api(`/collections/${c.id}/items/${editing!.id}`, added ? 'DELETE' : 'PUT');
                  await reload();
                  editing = items.find((m) => m.id === editing!.id) || null;
                });
              }}
              >{c.name}{#if editing.collections.includes(c.id)}<Check size={17} />{:else}<Plus
                  size={17}
                />{/if}</button
            >{:else}<p class="muted small">
              Crie uma coleção do mesmo tipo para organizar este arquivo.
            </p>{/each}
        </div>
        <button class="text-button danger" onclick={() => (modal = 'delete')}
          ><Trash2 size={16} />Excluir arquivo</button
        >
      {:else if modal === 'delete' && editing}<h2>Excluir este arquivo?</h2>
        <p>
          “{editing.name}” será removido do servidor e de todas as coleções{editing.path
            ? ', inclusive da unidade de rede'
            : ''}. {editing.backup
            ? 'A cópia no Google Drive continua lá e pode ser trazida de volta pela página Google Drive.'
            : 'Não há cópia no Google Drive: esta ação é permanente.'}
        </p>
        <button
          class="primary danger-bg"
          disabled={busy}
          onclick={() => {
            void action(async () => {
              await api(`/media/${editing!.id}`, 'DELETE');
              if (current?.id === editing!.id) current = null;
              await reload();
              editing = null;
              modal = '';
              notify('Arquivo excluído');
            });
          }}>Excluir deste servidor<Trash2 size={17} /></button
        ><button class="text-button" onclick={() => (modal = 'edit')}>Manter meu arquivo</button
        >{/if}
      {#if error}<div class="error" role="alert">{error}</div>{/if}
    </div>
  </div>
{/if}

{#if current}
  {#if current.kind === 'audio'}
    <section class="audio-dock" aria-label="Player de música">
      <div class="now-track">
        <span class="track-art"><Music2 size={24} /></span>
        <div>
          <strong>{current.name}</strong><small
            >{current.remote || current.cloud_only
              ? 'Vindo do Google Drive'
              : current.genre || 'Sua biblioteca pessoal'}</small
          >
        </div>
      </div>
      <div class="audio-controls">
        <div>
          <button
            class="icon-button"
            class:enabled={shuffled}
            aria-label="Alternar aleatório"
            aria-pressed={shuffled}
            onclick={() => (shuffled = !shuffled)}><Shuffle size={17} /></button
          ><button class="icon-button" aria-label="Anterior" onclick={() => next(-1)}
            ><SkipBack size={19} /></button
          ><button
            class="play-button"
            aria-label={playing ? 'Pausar' : 'Reproduzir'}
            onclick={togglePlay}
            >{#if playing}<Pause fill="currentColor" size={18} />{:else}<Play
                fill="currentColor"
                size={18}
              />{/if}</button
          ><button class="icon-button" aria-label="Próxima" onclick={() => next()}
            ><SkipForward size={19} /></button
          ><button
            class="icon-button"
            class:enabled={repeat}
            aria-label="Repetir faixa"
            aria-pressed={repeat}
            onclick={() => (repeat = !repeat)}><Repeat2 size={17} /></button
          >
        </div>
        <label class="seek"
          ><span>{clock(position)}</span><input
            aria-label="Posição da música"
            type="range"
            min="0"
            max={Number.isFinite(duration) ? duration : 0}
            step="1"
            value={compatible ? position - offset : position}
            oninput={(e) => {
              if (mediaElement) mediaElement.currentTime = Number(e.currentTarget.value);
            }}
          /><span>{clock(Number.isFinite(duration) ? duration : 0)}</span></label
        >
      </div>
      <div class="audio-right">
        <Volume2 size={18} /><input
          type="range"
          aria-label="Volume"
          min="0"
          max="1"
          step="0.05"
          value="1"
          oninput={(e) => {
            if (mediaElement) mediaElement.volume = Number(e.currentTarget.value);
          }}
        /><button class="icon-button" aria-label="Fechar player" onclick={closePlayer}
          ><X size={18} /></button
        >
      </div>
      {#key `${current.id}-${compatible}-${forceCpu}-${offset}`}<audio
          bind:this={mediaElement}
          data-media-id={current.id}
          data-offset={compatible ? offset : 0}
          src={playbackSource(current)}
          onloadedmetadata={loaded}
          ontimeupdate={timeUpdate}
          onplay={() => (playing = true)}
          onpause={(e) => {
            playing = false;
            void saveProgress(e.currentTarget);
          }}
          onended={ended}
          onerror={() =>
            (playerError = 'Este aparelho não consegue reproduzir o codec deste áudio.')}
        ></audio>{/key}{#if playerError}<div class="player-error" role="alert">
          {playerError}{#if !current.remote}<button
              class="secondary"
              onclick={() => useCompatible(compatible)}
              >Tentar modo compatível {compatible ? 'com CPU' : ''}</button
            >{/if}
        </div>{/if}
    </section>
  {:else if current.kind === 'photo'}<PhotoViewer
      photo={current}
      photos={queue}
      admin={(user?.admin ?? false) && !current.remote}
      tv={tvMode}
      onnavigate={(m) => (current = m)}
      onclose={closePlayer}
      onedit={(m) => edit(items.find((i) => i.id === m.id) ?? m)}
    />
  {:else}<div
      class="viewer"
      role="dialog"
      aria-modal="true"
      aria-label={current.name}
      tabindex="-1"
    >
      <header>
        <div>
          <span class="eyebrow">HOME CINEMA</span><strong>{current.name}</strong>
        </div>
        <button class="icon-button" aria-label="Fechar player" onclick={closePlayer}
          ><X size={25} /></button
        >
      </header>
      {#key `${current.id}-${compatible}-${forceCpu}-${offset}`}<video
          controls
          playsinline
          bind:this={mediaElement}
          data-media-id={current.id}
          data-offset={compatible ? offset : 0}
          src={playbackSource(current)}
          onloadedmetadata={loaded}
          ontimeupdate={timeUpdate}
          onplay={() => (playing = true)}
          onpause={(e) => {
            playing = false;
            void saveProgress(e.currentTarget);
          }}
          onended={ended}
          onerror={() =>
            (playerError = current?.remote
              ? 'Não foi possível reproduzir a partir do Google Drive. Verifique a conexão do servidor ou traga o arquivo de volta para usar o modo compatível.'
              : 'Não foi possível reproduzir. Tente o modo compatível; se já estiver ativo, tente CPU. A conversão requer FFmpeg no servidor.')}
          ><track kind="captions" /></video
        >{/key}
      <div class="viewer-bottom">
        {#if current.remote}<span
            ><Cloud size={16} />Vindo do Google Drive · pode demorar alguns segundos para começar</span
          >{:else}<span
            ><ShieldCheck size={16} />{compatible
              ? `Modo compatível · ${forceCpu ? 'CPU' : 'GPU se disponível'}`
              : 'Original · reprodução direta'}</span
          ><button class="secondary" onclick={() => useCompatible(compatible)}
            >Compatível {compatible ? '(CPU)' : ''}</button
          >{/if}<button class="secondary" onclick={() => next()}
          >Próximo vídeo<SkipForward size={17} /></button
        ><button
          class="icon-button"
          aria-label="Tela cheia"
          onclick={() => void mediaElement?.requestFullscreen()}><Maximize size={20} /></button
        >
      </div>
      {#if playerError}<div class="error" role="alert">{playerError}</div>{/if}
    </div>{/if}
{/if}
{#if toast}<div class="toast" role="status"><span><Check size={17} /></span>{toast}</div>{/if}
