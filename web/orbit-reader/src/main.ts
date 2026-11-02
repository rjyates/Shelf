/**
 * Shelf host for the Book Orbit EPUB reader.
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
        if (Number.isFinite(id) && id > 0) {
          return {
            path: `/reader/${id}/${id}`,
            query: { format: 'epub' },
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
