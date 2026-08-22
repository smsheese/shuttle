<script lang="ts">
  interface Props {
    open: boolean;
    src: string;
    filename: string;
    mime?: string;
    oncancel: () => void;
    onsend: (result: { data_base64: string; mime: string; filename: string }) => void;
    onskip?: () => void;
  }

  let { open, src, filename, mime = 'image/jpeg', oncancel, onsend, onskip }: Props = $props();

  type Tool = 'crop' | 'pen' | 'arrow' | 'pixelate';

  let canvasEl = $state<HTMLCanvasElement | null>(null);
  let tool = $state<Tool>('pen');
  let drawing = $state(false);
  let startX = $state(0);
  let startY = $state(0);
  let cropRect = $state<{ x: number; y: number; w: number; h: number } | null>(null);
  let img = $state<HTMLImageElement | null>(null);
  let busy = $state(false);

  const outputMime = $derived(mime.includes('png') ? 'image/png' : 'image/jpeg');

  $effect(() => {
    if (!open || !src) return;
    cropRect = null;
    tool = 'pen';
    const image = new Image();
    image.onload = () => {
      img = image;
      queueMicrotask(redraw);
    };
    image.src = src;
  });

  function redraw() {
    if (!canvasEl || !img) return;
    const maxW = Math.min(720, window.innerWidth - 48);
    const maxH = Math.min(520, window.innerHeight - 200);
    const scale = Math.min(maxW / img.width, maxH / img.height, 1);
    canvasEl.width = Math.round(img.width * scale);
    canvasEl.height = Math.round(img.height * scale);
    const ctx = canvasEl.getContext('2d');
    if (!ctx) return;
    ctx.clearRect(0, 0, canvasEl.width, canvasEl.height);
    ctx.drawImage(img, 0, 0, canvasEl.width, canvasEl.height);
    if (cropRect) {
      ctx.strokeStyle =
        getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() || '#3b82f6';
      ctx.lineWidth = 2;
      ctx.setLineDash([6, 4]);
      ctx.strokeRect(cropRect.x, cropRect.y, cropRect.w, cropRect.h);
      ctx.setLineDash([]);
    }
  }

  $effect(() => {
    img;
    cropRect;
    queueMicrotask(redraw);
  });

  function canvasPoint(e: PointerEvent) {
    const rect = canvasEl!.getBoundingClientRect();
    return {
      x: ((e.clientX - rect.left) / rect.width) * canvasEl!.width,
      y: ((e.clientY - rect.top) / rect.height) * canvasEl!.height,
    };
  }

  function onPointerDown(e: PointerEvent) {
    if (!canvasEl) return;
    drawing = true;
    const p = canvasPoint(e);
    startX = p.x;
    startY = p.y;
    if (tool === 'pen') {
      const ctx = canvasEl.getContext('2d');
      ctx?.beginPath();
      ctx!.strokeStyle = '#ef4444';
      ctx!.lineWidth = 3;
      ctx!.lineCap = 'round';
      ctx!.moveTo(p.x, p.y);
    } else if (tool === 'crop' || tool === 'pixelate') {
      cropRect = { x: p.x, y: p.y, w: 0, h: 0 };
    }
    canvasEl.setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!drawing || !canvasEl) return;
    const p = canvasPoint(e);
    if (tool === 'pen') {
      const ctx = canvasEl.getContext('2d');
      ctx?.lineTo(p.x, p.y);
      ctx?.stroke();
    } else if (tool === 'crop' || tool === 'pixelate') {
      cropRect = {
        x: Math.min(startX, p.x),
        y: Math.min(startY, p.y),
        w: Math.abs(p.x - startX),
        h: Math.abs(p.y - startY),
      };
    }
  }

  function onPointerUp(e: PointerEvent) {
    if (!drawing || !canvasEl) return;
    drawing = false;
    canvasEl.releasePointerCapture(e.pointerId);
    if (tool === 'arrow') {
      const p = canvasPoint(e);
      const ctx = canvasEl.getContext('2d');
      if (!ctx) return;
      ctx.strokeStyle = '#ef4444';
      ctx.fillStyle = '#ef4444';
      ctx.lineWidth = 3;
      ctx.beginPath();
      ctx.moveTo(startX, startY);
      ctx.lineTo(p.x, p.y);
      ctx.stroke();
      const angle = Math.atan2(p.y - startY, p.x - startX);
      const head = 12;
      ctx.beginPath();
      ctx.moveTo(p.x, p.y);
      ctx.lineTo(p.x - head * Math.cos(angle - 0.4), p.y - head * Math.sin(angle - 0.4));
      ctx.lineTo(p.x - head * Math.cos(angle + 0.4), p.y - head * Math.sin(angle + 0.4));
      ctx.closePath();
      ctx.fill();
    } else if (tool === 'pixelate' && cropRect && cropRect.w > 4 && cropRect.h > 4) {
      applyPixelate(cropRect);
      cropRect = null;
    }
  }

  function applyPixelate(rect: { x: number; y: number; w: number; h: number }) {
    if (!canvasEl) return;
    const ctx = canvasEl.getContext('2d');
    if (!ctx) return;
    const block = 10;
    const x = Math.floor(rect.x);
    const y = Math.floor(rect.y);
    const w = Math.floor(rect.w);
    const h = Math.floor(rect.h);
    const data = ctx.getImageData(x, y, w, h);
    for (let by = 0; by < h; by += block) {
      for (let bx = 0; bx < w; bx += block) {
        let r = 0;
        let g = 0;
        let b = 0;
        let a = 0;
        let n = 0;
        for (let dy = 0; dy < block && by + dy < h; dy++) {
          for (let dx = 0; dx < block && bx + dx < w; dx++) {
            const i = ((by + dy) * w + (bx + dx)) * 4;
            r += data.data[i];
            g += data.data[i + 1];
            b += data.data[i + 2];
            a += data.data[i + 3];
            n++;
          }
        }
        if (!n) continue;
        r = Math.round(r / n);
        g = Math.round(g / n);
        b = Math.round(b / n);
        a = Math.round(a / n);
        ctx.fillStyle = `rgba(${r},${g},${b},${a / 255})`;
        ctx.fillRect(x + bx, y + by, block, block);
      }
    }
  }

  function applyCrop() {
    if (!canvasEl || !cropRect || cropRect.w < 4 || cropRect.h < 4) return;
    const ctx = canvasEl.getContext('2d');
    if (!ctx) return;
    const { x, y, w, h } = cropRect;
    const cropped = ctx.getImageData(x, y, w, h);
    canvasEl.width = w;
    canvasEl.height = h;
    ctx.putImageData(cropped, 0, 0);
    cropRect = null;
    img = null;
  }

  function rotate90() {
    if (!canvasEl) return;
    const ctx = canvasEl.getContext('2d');
    if (!ctx) return;
    const w = canvasEl.width;
    const h = canvasEl.height;
    const tmp = document.createElement('canvas');
    tmp.width = h;
    tmp.height = w;
    const tctx = tmp.getContext('2d')!;
    tctx.translate(h / 2, w / 2);
    tctx.rotate(Math.PI / 2);
    tctx.drawImage(canvasEl, -w / 2, -h / 2);
    canvasEl.width = h;
    canvasEl.height = w;
    ctx.drawImage(tmp, 0, 0);
    img = null;
    cropRect = null;
  }

  async function exportAndSend() {
    if (!canvasEl || busy) return;
    busy = true;
    try {
      const blob = await new Promise<Blob | null>((resolve) =>
        canvasEl!.toBlob((b) => resolve(b), outputMime, 0.92)
      );
      if (!blob) return;
      const dataUrl = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(String(reader.result));
        reader.onerror = () => reject(reader.error);
        reader.readAsDataURL(blob);
      });
      const comma = dataUrl.indexOf(',');
      onsend({
        data_base64: comma >= 0 ? dataUrl.slice(comma + 1) : dataUrl,
        mime: outputMime,
        filename: filename.replace(/\.\w+$/, '') + (outputMime.includes('png') ? '.png' : '.jpg'),
      });
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <div class="backdrop" role="presentation" onclick={oncancel}></div>
  <div class="editor" role="dialog" aria-modal="true" aria-label="Edit image">
    <header class="head">
      <h2>Edit image</h2>
      <div class="tools">
        <button type="button" class:active={tool === 'crop'} onclick={() => (tool = 'crop')}>Crop</button>
        {#if tool === 'crop' && cropRect}
          <button type="button" class="apply" onclick={applyCrop}>Apply crop</button>
        {/if}
        <button type="button" onclick={rotate90}>Rotate 90°</button>
        <button type="button" class:active={tool === 'pen'} onclick={() => (tool = 'pen')}>Pen</button>
        <button type="button" class:active={tool === 'arrow'} onclick={() => (tool = 'arrow')}>Arrow</button>
        <button type="button" class:active={tool === 'pixelate'} onclick={() => (tool = 'pixelate')}>Blur</button>
      </div>
    </header>
    <div class="canvas-wrap">
      <canvas
        bind:this={canvasEl}
        onpointerdown={onPointerDown}
        onpointermove={onPointerMove}
        onpointerup={onPointerUp}
        onpointerleave={onPointerUp}
      ></canvas>
    </div>
    <footer class="actions">
      <button type="button" class="ghost" onclick={oncancel} disabled={busy}>Cancel</button>
      {#if onskip}
        <button type="button" class="ghost" onclick={onskip} disabled={busy}>Send without editing</button>
      {/if}
      <button type="button" class="primary" onclick={exportAndSend} disabled={busy}>
        {busy ? 'Preparing…' : 'Apply & Send'}
      </button>
    </footer>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.65);
    z-index: 230;
  }

  .editor {
    position: fixed;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    z-index: 231;
    width: min(760px, calc(100vw - 24px));
    max-height: calc(100vh - 24px);
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px;
    background: var(--bg-panel);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-md, 0 16px 48px rgba(0, 0, 0, 0.25));
  }

  .head {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  h2 {
    margin: 0;
    font-size: 16px;
  }

  .tools {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .tools button {
    border: 1px solid var(--border);
    background: var(--bg-input);
    color: var(--text);
    border-radius: var(--radius-sm);
    padding: 5px 10px;
    font-size: 12px;
    cursor: pointer;
  }

  .tools button.active,
  .tools button.apply {
    border-color: var(--accent);
    background: var(--accent-muted);
  }

  .canvas-wrap {
    overflow: auto;
    display: flex;
    justify-content: center;
    background: var(--bg-main);
    border-radius: var(--radius-sm);
    padding: 8px;
    min-height: 120px;
  }

  canvas {
    cursor: crosshair;
    touch-action: none;
    max-width: 100%;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 8px;
  }

  .actions button {
    border: none;
    border-radius: var(--radius-sm);
    padding: 8px 14px;
    font-size: 13px;
    cursor: pointer;
  }

  .actions button.ghost {
    background: var(--bg-hover);
    color: var(--text-secondary);
  }

  .actions button.primary {
    background: var(--accent);
    color: white;
    font-weight: 600;
  }

  .actions button:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
