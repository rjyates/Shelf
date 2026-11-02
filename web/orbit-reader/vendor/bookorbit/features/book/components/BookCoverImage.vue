<script setup lang="ts">
import { computed } from 'vue'
import { useCoverVersions } from '../composables/useCoverVersions'

const props = defineProps<{
  bookId: number
  type?: 'thumbnail' | 'cover'
  version?: string | number | Date | null
  class?: string
  alt?: string
}>()

const { coverUrl } = useCoverVersions()
const src = computed(() => coverUrl(props.bookId, props.type ?? 'thumbnail', props.version))
</script>

<template>
  <img :src="src" :class="props.class" :alt="props.alt ?? ''" loading="lazy" />
</template>
