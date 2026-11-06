<!--
  Shelf shell for the Book Orbit reader. No Shelf theming is applied here.
  Upstream reader: https://github.com/bookorbit/bookorbit (AGPL-3.0-only)
-->
<script setup lang="ts">
import { Toaster } from '@/components/ui/sonner'
import TtsMiniPlayer from '@/features/tts/components/TtsMiniPlayer.vue'
import MediaOverlayMiniPlayer from '@/features/reader/media-overlay/components/MediaOverlayMiniPlayer.vue'
</script>

<template>
  <RouterView />
  <TtsMiniPlayer />
  <MediaOverlayMiniPlayer />
  <Toaster />
</template>
