/**
 * Shelf host for the Book Orbit reader (EPUB + PDF + CBZ).
 * Reader implementation: https://github.com/bookorbit/bookorbit (AGPL-3.0-only)
 */
import { createApp } from 'vue'
import { createRouter, createWebHistory } from 'vue-router'
import { i18n } from '@/i18n'
import App from './App.vue'
import ReaderRoute from './ReaderRoute.vue'
import '@/assets/main.css'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/reader',
      redirect: (to) => {
        const id = Number(to.query.id)
        const raw =
          typeof to.query.format === 'string' ? to.query.format.toLowerCase() : 'epub'
        const format =
          raw === 'pdf' ||
          raw === 'cbz' ||
          raw === 'cbr' ||
          raw === 'cb7' ||
          raw === 'djvu' ||
          raw === 'mp3' ||
          raw === 'm4b' ||
          raw === 'aax' ||
          raw === 'opus' ||
          raw === 'ogg' ||
          raw === 'flac' ||
          raw === 'audiozip'
            ? raw
            : 'epub'
        if (Number.isFinite(id) && id > 0) {
          return {
            path: `/reader/${id}/${id}`,
            query: { format },
          }
        }
        return { path: '/' }
      },
    },
    {
      path: '/reader/:bookId/:fileId?',
      name: 'reader',
      component: ReaderRoute,
    },
  ],
})

createApp(App).use(router).use(i18n).mount('#app')
