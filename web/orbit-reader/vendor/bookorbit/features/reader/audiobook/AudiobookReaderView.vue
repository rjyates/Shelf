<!--
  Shelf stub: audiobook playback is not opened yet.
  Placeholder so Book Orbit's ReaderView.vue can keep its format switch.
  Upstream: https://github.com/bookorbit/bookorbit (AGPL-3.0-only)
-->
<script setup lang="ts">
defineProps<{ bookId: number; fileId: number }>()
</script>
<template>
  <div class="flex h-full items-center justify-center p-6 text-sm text-muted-foreground">
    Audiobook playback is not available in Shelf yet.
  </div>
</template>
