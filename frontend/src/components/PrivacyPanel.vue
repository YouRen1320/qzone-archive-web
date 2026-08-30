<script setup lang="ts">
// This compact disclosure stays visible near the action that handles account credentials.
defineProps<{ compact?: boolean }>()
</script>

<template>
  <aside class="privacy-panel" :class="{ compact }">
    <div class="privacy-icon" aria-hidden="true">⌁</div>
    <div>
      <strong>一次一人，用完即删</strong>
      <p>QQ Cookie 只在服务器内存中使用；每个任务拥有独立临时目录和 SQLite，下载后自动清理。</p>
    </div>
  </aside>
</template>
