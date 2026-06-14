export default defineNuxtConfig({
  compatibilityDate: '2026-06-14',
  modules: ['@nuxtjs/robots'],
  css: ['~/assets/styles/main.less'],
  nitro: {
    preset: 'static',
  },
  app: {
    head: {
      htmlAttrs: {
        lang: 'zh-CN',
      },
      title: '小语文稿 MoekaNotes',
      meta: [
        { name: 'viewport', content: 'width=device-width, initial-scale=1' },
        {
          name: 'description',
          content: '小语文稿 MoekaNotes 是一款优雅、流畅、本地优先的 Markdown 编辑应用。',
        },
        { property: 'og:title', content: '小语文稿 MoekaNotes' },
        {
          property: 'og:description',
          content: '言渡四海，意会千山。为重度写作者打造的高性能 Markdown 编辑体验。',
        },
        { property: 'og:type', content: 'website' },
      ],
    },
  },
  vite: {
    css: {
      preprocessorOptions: {
        less: {
          math: 'always',
        },
      },
    },
  },
})
