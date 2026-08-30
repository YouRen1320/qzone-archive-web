import { createApp } from 'vue'
import App from './App.vue'
import './styles.css'

// The application is intentionally a single focused workflow; server state is owned by useArchiveJob.
createApp(App).mount('#app')
