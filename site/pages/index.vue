<script setup lang="ts">
const navItems = ['简介', '特性', '需求', '下载']
</script>

<template>
  <main class="site-shell">
    <section class="hero" aria-labelledby="hero-title">
      <header class="topbar" aria-label="主导航">
        <nav class="nav-pill">
          <a
            v-for="item in navItems"
            :key="item"
            :class="['nav-link', { active: item === '简介' }]"
            :href="`#${item}`"
          >
            {{ item }}
          </a>
        </nav>

        <button class="language-pill" type="button" aria-label="切换语言">
          <img src="../assets/icon-lang.svg" alt="" aria-hidden="true">
          简体中文
        </button>
      </header>

      <div id="简介" class="hero-content">
        <img class="flower flower-left" src="../assets/flower-left.svg" alt="" aria-hidden="true">
        <img class="brand-logo" src="../assets/logo.svg" alt="小语文稿" aria-describedby="hero-tagline">
        <img class="flower flower-right" src="../assets/flower-right.svg" alt="" aria-hidden="true">

        <h1 id="hero-title" class="sr-only">小语文稿</h1>

        <p id="hero-tagline" class="tagline">
          <span>言渡四海</span>
          <span>意会千山</span>
        </p>

        <div class="hero-actions" aria-label="主要操作">
          <a class="button button-dark" href="#下载">
            <img src="../assets/icon-download.svg" alt="" aria-hidden="true">
            应用下载
          </a>
          <a class="button button-red" href="#手册">
            <img src="../assets/icon-help.svg" alt="" aria-hidden="true">
            帮助手册
          </a>
        </div>
      </div>
    </section>

    <section id="特性" class="feature-one" aria-labelledby="feature-title">
      <div class="feature-heading">
        <img src="../assets/icon-feature-1.svg" alt="" aria-hidden="true">
        <div>
          <h2 id="feature-title">高性能 Markdown 编辑体验</h2>
          <p>基于 GPU 渲染的编辑器内核，跟随高刷新率屏幕</p>
        </div>
      </div>

      <img class="product-screenshot" src="../assets/screenshot-feature-1.png" alt="小语文稿应用界面截图">
    </section>
  </main>
</template>
