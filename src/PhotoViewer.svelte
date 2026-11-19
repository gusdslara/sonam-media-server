<script lang="ts">
  import { onDestroy } from 'svelte';
  import {
    X,
    Info,
    Download,
    Pencil,
    ChevronLeft,
    ChevronRight,
    MapPin,
    Camera,
    Calendar,
    Image as ImageIcon,
    Copy,
    ExternalLink,
    Maximize,
    Minimize,
    HardDrive,
    Tag,
    LoaderCircle
  } from '@lucide/svelte';
  import { api, source, size, copyText } from './lib';
  import type { Media, MediaDetails } from './lib';

  let {
    photo,
    photos,
    admin,
    tv = false,
    onnavigate,
    onclose,
    onedit
  }: {
    photo: Media;
    photos: Media[];
    admin: boolean;
    tv?: boolean;
    onnavigate: (m: Media) => void;
    onclose: () => void;
    onedit: (m: Media) => void;
  } = $props();

  const reduced =
    typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  const storedInfo = (() => {
    try {
      return localStorage.getItem('hs-photo-info') === '1';
    } catch {
      return false;
    }
  })();

  let root = $state<HTMLDivElement>();
  let chrome = $state(true);
  let info = $state(storedInfo && innerWidth > 900);
  let details = $state<Record<string, MediaDetails>>({});
  let detailsError = $state('');
  let copied = $state(false);
  let isFullscreen = $state(false);
  // Gesture state
  let dx = $state(0),
    dy = $state(0),
    scale = $state(1),
    tx = $state(0),
    ty = $state(0),
    animating = $state(false),
    loading = $state(true);

  const index = $derived(photos.findIndex((p) => p.id === photo.id));
  const prev = $derived(index > 0 ? photos[index - 1] : null);
  const nextPhoto = $derived(index >= 0 && index < photos.length - 1 ? photos[index + 1] : null);
  const current = $derived(details[photo.id]);

  // Reset zoom when the photo changes and warm up neighbours.
  $effect(() => {
    void photo.id;
    scale = 1;
    tx = 0;
    ty = 0;
    loading = true;
    for (const n of [prev, nextPhoto]) if (n) new Image().src = source(n);
  });

  $effect(() => {
    if (info && !details[photo.id]) void load(photo.id);
  });

  async function load(id: string) {
    detailsError = '';
    try {
      const d = await api<MediaDetails>(
        photo.remote ? `/cloud/items/${id}/details` : `/media/${id}/details`
      );
      details = { ...details, [id]: d };
    } catch (e) {
      detailsError = (e as Error).message;
    }
  }

  function setInfo(open: boolean) {
    info = open;
    try {
      localStorage.setItem('hs-photo-info', open ? '1' : '0');
    } catch {
      /* private mode */
    }
  }

  function go(delta: number) {
    const target = delta > 0 ? nextPhoto : prev;
    if (!target) {
      // Rubber-band at the ends.
      animating = true;
      dx = -delta * 40;
      setTimeout(() => (dx = 0), 120);
      setTimeout(() => (animating = false), 300);
      return;
    }
    if (reduced) {
      onnavigate(target);
      return;
    }
    animating = true;
    dx = -delta * innerWidth;
    setTimeout(() => {
      animating = false;
      dx = delta * innerWidth * 0.35;
      onnavigate(target);
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          animating = true;
          dx = 0;
          setTimeout(() => (animating = false), 260);
        })
      );
    }, 180);
  }

  // ---- Pointer gestures: swipe, swipe down to close, swipe up for info,
  // pinch and double tap to zoom, drag to pan while zoomed.
  const pointers = new Map<number, { x: number; y: number }>();
  let start = { x: 0, y: 0, t: 0, scale: 1, tx: 0, ty: 0, dist: 0 };
  let axis: '' | 'x' | 'y' = '';
  let lastTap = 0;
  let tapTimer: ReturnType<typeof setTimeout> | undefined;
  onDestroy(() => clearTimeout(tapTimer));

  const distance = () => {
    const [a, b] = [...pointers.values()];
    return Math.hypot(a.x - b.x, a.y - b.y);
  };

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    start = {
      x: e.clientX,
      y: e.clientY,
      t: performance.now(),
      scale,
      tx,
      ty,
      dist: pointers.size === 2 ? distance() : 0
    };
    axis = '';
  }

  function move(e: PointerEvent) {
    if (!pointers.has(e.pointerId)) return;
    pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.size === 2) {
      if (!start.dist) start.dist = distance();
      scale = Math.min(4, Math.max(1, (start.scale * distance()) / start.dist));
      if (scale === 1) tx = ty = 0;
      return;
    }
    const mx = e.clientX - start.x,
      my = e.clientY - start.y;
    if (scale > 1) {
      tx = start.tx + mx;
      ty = start.ty + my;
      return;
    }
    if (!axis && Math.hypot(mx, my) > 10) axis = Math.abs(mx) > Math.abs(my) ? 'x' : 'y';
    if (axis === 'x') dx = mx;
    else if (axis === 'y') dy = my;
  }

  function up(e: PointerEvent) {
    if (!pointers.has(e.pointerId)) return;
    pointers.delete(e.pointerId);
    if (pointers.size) return;
    const mx = e.clientX - start.x,
      my = e.clientY - start.y;
    const elapsed = performance.now() - start.t;
    const moved = Math.hypot(mx, my);
    if (scale <= 1 && axis === 'x') {
      const fast = Math.abs(mx) / elapsed > 0.5;
      if (Math.abs(mx) > Math.min(110, innerWidth * 0.2) || (fast && Math.abs(mx) > 30))
        go(mx < 0 ? 1 : -1);
      else snap();
    } else if (scale <= 1 && axis === 'y') {
      if (my > 120) onclose();
      else {
        if (my < -70) setInfo(true);
        snap();
      }
    } else if (moved < 8 && elapsed < 300) tap(e);
  }

  function snap() {
    animating = !reduced;
    dx = 0;
    dy = 0;
    setTimeout(() => (animating = false), 240);
  }

  function tap(e: PointerEvent) {
    const now = performance.now();
    if (now - lastTap < 300) {
      clearTimeout(tapTimer);
      lastTap = 0;
      toggleZoom(e.clientX, e.clientY);
      return;
    }
    lastTap = now;
    tapTimer = setTimeout(() => (chrome = !chrome), 260);
  }

  function toggleZoom(x: number, y: number) {
    animating = !reduced;
    if (scale > 1) {
      scale = 1;
      tx = ty = 0;
    } else {
      scale = 2.5;
      tx = (innerWidth / 2 - x) * 1.5;
      ty = (innerHeight / 2 - y) * 1.5;
    }
    setTimeout(() => (animating = false), 240);
  }

  function wheel(e: WheelEvent) {
    if (!e.ctrlKey && Math.abs(e.deltaY) < 40) return;
    e.preventDefault();
    scale = Math.min(4, Math.max(1, scale * (e.deltaY < 0 ? 1.15 : 0.87)));
    if (scale === 1) tx = ty = 0;
  }

  function keydown(e: KeyboardEvent) {
    if (e.target instanceof HTMLElement && ['INPUT', 'TEXTAREA'].includes(e.target.tagName)) return;
    if (e.key === 'ArrowRight') {
      e.preventDefault();
      e.stopImmediatePropagation();
      go(1);
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      e.stopImmediatePropagation();
      go(-1);
    } else if (e.key === 'i' || (tv && e.key === 'ArrowUp')) {
      e.preventDefault();
      e.stopImmediatePropagation();
      setInfo(e.key === 'ArrowUp' ? true : !info);
    } else if (tv && e.key === 'ArrowDown') {
      e.preventDefault();
      e.stopImmediatePropagation();
      setInfo(false);
    }
  }

  async function toggleFullscreen() {
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await root?.requestFullscreen();
    } catch {
      /* not supported (iOS Safari) */
    }
  }

  // ---- Formatting
  const longDate = new Intl.DateTimeFormat('pt-BR', {
    weekday: 'short',
    day: 'numeric',
    month: 'long',
    year: 'numeric',
    timeZone: 'UTC'
  });
  const hour = new Intl.DateTimeFormat('pt-BR', {
    hour: '2-digit',
    minute: '2-digit',
    timeZone: 'UTC'
  });
  const added = (n: number) =>
    new Intl.DateTimeFormat('pt-BR', { dateStyle: 'long', timeStyle: 'short' }).format(n * 1000);
  const num = (n: number, digits = 1) =>
    n.toLocaleString('pt-BR', { maximumFractionDigits: digits });
  function dms(value: number, pos: string, neg: string) {
    const a = Math.abs(value);
    const d = Math.floor(a),
      m = Math.floor((a - d) * 60),
      s = Math.round(((a - d) * 60 - m) * 60);
    return `${d}°${String(m).padStart(2, '0')}′${String(s).padStart(2, '0')}″ ${value < 0 ? neg : pos}`;
  }
  const kindLabel = (mime: string) => mime.split('/')[1]?.toUpperCase().replace('JPEG', 'JPG');

  async function copyCoords(lat: number, lon: number) {
    await copyText(`${lat}, ${lon}`);
    copied = true;
    setTimeout(() => (copied = false), 1800);
  }
</script>

<svelte:window onkeydown={keydown} />
<svelte:document onfullscreenchange={() => (isFullscreen = !!document.fullscreenElement)} />

<div
  bind:this={root}
  class="viewer pv"
  class:info-open={info}
  class:chrome-hidden={!chrome}
  role="dialog"
  aria-modal="true"
  aria-label={`${photo.name}, foto ${index + 1} de ${photos.length}`}
  tabindex="-1"
  style={`--fade:${Math.max(0.35, 1 - Math.abs(dy) / 500)}`}
>
  <div
    class="pv-stage"
    role="presentation"
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={up}
    onwheel={wheel}
    ondblclick={(e) => toggleZoom(e.clientX, e.clientY)}
  >
    {#if loading}<span class="pv-loading" aria-hidden="true"
        ><LoaderCircle size={28} class="spin" /></span
      >{/if}
    {#key photo.id}
      <img
        class="pv-img"
        class:animating
        src={source(photo)}
        alt={photo.name}
        draggable="false"
        onload={() => (loading = false)}
        onerror={() => (loading = false)}
        style={`transform: translate3d(${dx + tx}px, ${dy + ty}px, 0) scale(${scale})`}
      />
    {/key}
  </div>

  <header class="pv-top">
    <button class="pv-icon" aria-label="Fechar" onclick={onclose}><X size={22} /></button>
    <div class="pv-title">
      <strong>{photo.name}</strong>
      <span>{index + 1} de {photos.length}</span>
    </div>
    <a
      class="pv-icon"
      href={source(photo)}
      download={photo.name}
      aria-label="Baixar original"
      title="Baixar original"><Download size={20} /></a
    >
    <button
      class="pv-icon desktop-only"
      aria-label={isFullscreen ? 'Sair da tela cheia' : 'Tela cheia'}
      title="Tela cheia"
      onclick={toggleFullscreen}
      >{#if isFullscreen}<Minimize size={20} />{:else}<Maximize size={20} />{/if}</button
    >
    {#if admin}<button
        class="pv-icon"
        aria-label="Editar nome, gênero e coleções"
        title="Editar"
        onclick={() => onedit(photo)}><Pencil size={19} /></button
      >{/if}
    <button
      class="pv-icon"
      class:active={info}
      aria-label="Detalhes da foto"
      aria-pressed={info}
      title="Detalhes (i)"
      onclick={() => setInfo(!info)}><Info size={21} /></button
    >
  </header>

  {#if prev}<button class="pv-nav prev" aria-label="Foto anterior" onclick={() => go(-1)}
      ><ChevronLeft size={30} /></button
    >{/if}
  {#if nextPhoto}<button class="pv-nav next" aria-label="Próxima foto" onclick={() => go(1)}
      ><ChevronRight size={30} /></button
    >{/if}

  {#if info}
    <aside class="pv-info" aria-label="Detalhes da foto">
      <div class="pv-info-head">
        <span class="pv-handle" aria-hidden="true"></span>
        <h2>Detalhes</h2>
        <button class="pv-icon" aria-label="Fechar detalhes" onclick={() => setInfo(false)}
          ><X size={19} /></button
        >
      </div>
      {#if !current}
        {#if detailsError}<p class="pv-muted">{detailsError}</p>{:else}
          <div class="pv-skeleton" aria-label="Carregando detalhes">
            <span></span><span></span><span></span>
          </div>{/if}
      {:else}
        {@const p = current.photo}
        <p class="pv-name">{current.name}</p>
        <ul class="pv-facts">
          <li>
            <Calendar size={20} />
            <div>
              {#if p?.taken}
                <strong>{longDate.format(new Date(p.taken + 'Z'))}</strong>
                <span
                  >{hour.format(new Date(p.taken + 'Z'))}{p.taken_offset
                    ? ` · UTC${p.taken_offset}`
                    : ''} · data da câmera</span
                >
              {:else}
                <strong>{added(current.created)}</strong>
                <span>Data de envio (a foto não traz data de captura)</span>
              {/if}
            </div>
          </li>
          {#if p?.make || p?.model}
            <li>
              <Camera size={20} />
              <div>
                <strong>{[p.make, p.model].filter(Boolean).join(' ')}</strong>
                {#if p.lens}<span>{p.lens}</span>{/if}
                <span class="pv-chips">
                  {#if p.aperture}<em>ƒ/{num(p.aperture, 2)}</em>{/if}
                  {#if p.exposure}<em>{p.exposure.replace('.', ',')}</em>{/if}
                  {#if p.iso}<em>ISO {p.iso}</em>{/if}
                  {#if p.focal_length}<em
                      >{num(p.focal_length)} mm{p.focal_length_35mm
                        ? ` (${p.focal_length_35mm} mm equiv.)`
                        : ''}</em
                    >{/if}
                  {#if p.flash}<em>Flash</em>{/if}
                </span>
              </div>
            </li>
          {/if}
          <li>
            <ImageIcon size={20} />
            <div>
              <strong
                >{#if p?.width && p?.height}{p.width} × {p.height} · {num(
                    (p.width * p.height) / 1e6
                  )} MP{:else}{kindLabel(current.mime)}{/if}</strong
              >
              <span>{size(current.size)} · {kindLabel(current.mime)}</span>
            </div>
          </li>
          {#if p?.location}
            {@const loc = p.location}
            <li class="pv-location">
              <MapPin size={20} />
              <div>
                <strong>{dms(loc.latitude, 'N', 'S')}, {dms(loc.longitude, 'L', 'O')}</strong>
                {#if loc.altitude !== null}<span>{loc.altitude} m de altitude</span>{/if}
                <div class="pv-map" aria-hidden="true">
                  <svg viewBox="0 0 240 90" preserveAspectRatio="none"
                    ><path d="M0 60 Q60 20 120 45 T240 30" /><path
                      d="M0 80 Q80 50 160 70 T240 55"
                    /><path d="M40 0 L70 90" /><path d="M170 0 L150 90" /></svg
                  >
                  <span class="pv-pin"><MapPin size={22} /></span>
                </div>
                <span class="pv-actions">
                  <a
                    class="secondary"
                    href={`https://www.openstreetmap.org/?mlat=${loc.latitude}&mlon=${loc.longitude}#map=16/${loc.latitude}/${loc.longitude}`}
                    target="_blank"
                    rel="noopener noreferrer"><ExternalLink size={15} />Abrir no mapa</a
                  >
                  <button class="secondary" onclick={() => copyCoords(loc.latitude, loc.longitude)}
                    ><Copy size={15} />{copied ? 'Copiado' : 'Copiar'}</button
                  >
                </span>
                <span class="pv-note"
                  >A localização fica na sua casa. O mapa só abre (na OpenStreetMap) se você tocar.</span
                >
              </div>
            </li>
          {/if}
          {#if current.genre || current.collections.length}
            <li>
              <Tag size={20} />
              <div>
                <strong>{current.genre || 'Sem gênero'}</strong>
                {#if current.collections.length}<span class="pv-chips"
                    >{#each current.collections as c (c)}<em>{c}</em>{/each}</span
                  >{/if}
              </div>
            </li>
          {/if}
          {#if current.path}
            <li>
              <HardDrive size={20} />
              <div>
                <strong>Na unidade de rede</strong>
                <span class="pv-path">{current.path}</span>
              </div>
            </li>
          {/if}
        </ul>
        {#if p?.software}<p class="pv-muted">Processada por {p.software}</p>{/if}
        <p class="pv-muted">Adicionada em {added(current.created)}</p>
      {/if}
    </aside>
  {/if}
</div>
