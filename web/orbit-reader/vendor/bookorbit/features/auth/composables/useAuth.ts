/**
 * Shelf stub for Book Orbit's useAuth.
 * Derived from Book Orbit (AGPL-3.0-only): https://github.com/bookorbit/bookorbit
 *
 * Shelf uses cookie sessions and does not sync reader preferences to the server yet.
 */
import { computed, ref } from 'vue'
import type { AuthUser } from '@bookorbit/types'

const user = ref<AuthUser | null>({
  id: 1,
  username: 'shelf',
  name: 'Shelf',
  active: true,
  isSuperuser: true,
  isDefaultPassword: false,
  provisioningMethod: 'local',
  permissions: [],
  settings: {
    syncReaderPreferences: false,
  },
} as AuthUser)

const isLoading = ref(false)

export function useAuth() {
  return {
    user,
    isLoading,
    isAuthenticated: computed(() => user.value != null),
    login: async () => user.value,
    logout: async () => {
      window.location.href = '/'
    },
    fetchSession: async () => user.value,
  }
}
