<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { cursorPosition } from '@tauri-apps/api/window';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { onDestroy, onMount } from 'svelte';
  import { createDragController } from './drag-controller';
  import { percentRegions, type HitRegion, type HitLayoutPayload } from './hit-layout';

  interface InteractionState {
    position_locked: boolean;
    click_through: boolean;
  }

  const DRAG_THRESHOLD_PHYSICAL_PX = 3;

  let isDragging = false;
  let pointerActive = false;
  let activePointerId: number | null = null;
  let startX = 0;
  let startY = 0;
  let clickCount = 0;
  let clickTimer: ReturnType<typeof setTimeout> | null = null;
  let positionLocked = false;
  let promptedLockedMenu = false;
  let regions: HitRegion[] = [];
  let pointerAlpha = 0;
  let unlistenInteraction: UnlistenFn | null = null;
  let unlistenLayout: UnlistenFn | null = null;
  const drag = createDragController({
    start: point => invoke<number>('drag_start', { x: point.x, y: point.y }),
    move: dragId => invoke('drag_move', { dragId }),
    end: (dragId, point, cancelled) => invoke('drag_end', {
      dragId, x: point?.x ?? null, y: point?.y ?? null, cancelled,
    }),
    cursor: cursorPosition,
    frame: callback => requestAnimationFrame(callback),
    cancelFrame: cancelAnimationFrame,
    error: error => console.error('Clyde drag:', error),
  });

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return; // Only handle left click — right click goes to onContextMenu
    isDragging = false;
    pointerActive = true;
    activePointerId = e.pointerId;
    promptedLockedMenu = false;
    startX = e.screenX;
    startY = e.screenY;
    // Capture pointer so events keep flowing even if cursor leaves the hit window
    // (critical for mini mode where the hit window is very small).
    (e.target as Element)?.setPointerCapture(e.pointerId);
    if (positionLocked) return;

    drag.start();

    clickCount++;
    if (clickTimer) clearTimeout(clickTimer);
    clickTimer = setTimeout(() => {
      const count = clickCount;
      clickCount = 0;
      if (!isDragging) {
        if (count === 2) invoke('hit_double_click');
        else if (count >= 4) invoke('hit_flail');
      }
    }, 300);
  }

  function dragDistanceThresholdLogicalPx() {
    return DRAG_THRESHOLD_PHYSICAL_PX / (window.devicePixelRatio || 1);
  }

  function onPointerMove(e: PointerEvent) {
    if (!pointerActive) return;
    if (activePointerId !== null && e.pointerId !== activePointerId) return;
    if (e.buttons === 0) return;
    if (positionLocked) {
      const dx = e.screenX - startX;
      const dy = e.screenY - startY;
      if (!promptedLockedMenu && Math.sqrt(dx * dx + dy * dy) >= dragDistanceThresholdLogicalPx()) {
        promptedLockedMenu = true;
        invoke('show_context_menu');
      }
      return;
    }
    // Mark as dragging after crossing the same physical-pixel threshold as Rust.
    if (!isDragging) {
      const dx = e.screenX - startX;
      const dy = e.screenY - startY;
      if (Math.sqrt(dx * dx + dy * dy) >= dragDistanceThresholdLogicalPx()) {
        isDragging = true;
        clickCount = 0;
        if (clickTimer) clearTimeout(clickTimer);
      }
    }
    drag.move();
  }

  function endPointer(e: PointerEvent, cancelled = false) {
    if (!pointerActive) return;
    if (activePointerId !== null && e.pointerId !== activePointerId) return;
    const target = e.target as Element;
    pointerActive = false;
    activePointerId = null;
    promptedLockedMenu = false;
    if (target?.hasPointerCapture(e.pointerId)) target.releasePointerCapture(e.pointerId);
    if (cancelled && clickTimer) {
      clearTimeout(clickTimer);
      clickCount = 0;
    }
    void drag.end(cancelled);
  }

  function onPointerUp(e: PointerEvent) {
    endPointer(e);
  }

  function onPointerCancel(e: PointerEvent) {
    endPointer(e, true);
  }

  function onLostPointerCapture(e: PointerEvent) {
    if (pointerActive) endPointer(e, true);
  }

  function onKeyDown(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      invoke('hit_double_click');
    }
  }

  function onContextMenu(e: MouseEvent) {
    e.preventDefault();
    invoke('show_context_menu');
  }

  onMount(() => {
    const setup = async () => {
      const interaction = await invoke<InteractionState>('get_interaction_state');
      positionLocked = interaction.position_locked ?? false;

      unlistenInteraction = await listen<InteractionState>('interaction-state-changed', ({ payload }) => {
        positionLocked = payload.position_locked ?? false;
      });

      unlistenLayout = await listen<HitLayoutPayload>('hit-layout-changed', ({ payload }) => {
        regions = percentRegions(payload);
        pointerAlpha = payload.pointer_alpha ?? 0;
      });

      const initialLayout = await invoke<HitLayoutPayload | null>('get_current_hit_layout');
      if (initialLayout) {
        regions = percentRegions(initialLayout);
        pointerAlpha = initialLayout.pointer_alpha ?? 0;
      }

      window.addEventListener('pointermove', onPointerMove);
      window.addEventListener('pointerup', onPointerUp);
      window.addEventListener('pointercancel', onPointerCancel);
    };
    setup();
  });

  onDestroy(() => {
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerup', onPointerUp);
    window.removeEventListener('pointercancel', onPointerCancel);
    void drag.end(true);
    unlistenInteraction?.();
    unlistenLayout?.();
    if (clickTimer) clearTimeout(clickTimer);
  });
</script>

<div class="hit-root" style={`--pointer-alpha:${pointerAlpha}`}>
  {#each regions as region, index (index)}
    <div
      class="hit-zone"
      class:locked={positionLocked}
      style:left={`${region.x}%`}
      style:top={`${region.y}%`}
      style:width={`${region.width}%`}
      style:height={`${region.height}%`}
      onpointerdown={onPointerDown}
      onlostpointercapture={onLostPointerCapture}
      oncontextmenu={onContextMenu}
      onkeydown={onKeyDown}
      role="button"
      tabindex="0"
      aria-label="Clyde desktop pet"
    ></div>
  {/each}
</div>

<style>
  .hit-root {
    position: relative;
    width: 100%;
    height: 100%;
    background: transparent;
    pointer-events: none;
  }

  .hit-zone {
    position: absolute;
    background: rgba(0, 0, 0, var(--pointer-alpha, 0));
    cursor: grab;
    pointer-events: auto;
    touch-action: none;
    user-select: none;
    -webkit-user-select: none;
  }

  .hit-zone.locked {
    cursor: not-allowed;
  }

  .hit-zone:focus:not(:focus-visible) {
    outline: none;
  }
</style>
