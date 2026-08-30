<script setup lang="ts">
import { useId } from 'vue'

// This disclosure stays next to credential-handling actions and uses a unique label for future reuse.
defineProps<{ compact?: boolean }>()

const titleId = useId()
</script>

<template>
  <div class="privacy-panel" :class="{ compact }" role="note" :aria-labelledby="titleId">
    <div class="privacy-icon" aria-hidden="true"><span></span></div>
    <div>
      <strong :id="titleId">一次一人，用完即删</strong>
      <p>QQ Cookie 只在服务器内存中使用；每个任务拥有独立临时目录和 SQLite，下载后自动清理。</p>
    </div>
  </div>
</template>
