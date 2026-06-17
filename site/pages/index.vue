<script setup lang="ts">
import { computed, ref } from 'vue'

import iconFeature1 from '../assets/icon-feature-1.svg'
import iconFeature2 from '../assets/icon-feature-2.svg'
import iconFeature3 from '../assets/icon-feature-3.svg'
import iconFeature4 from '../assets/icon-feature-4.svg'
import iconFeature5 from '../assets/icon-feature-5.svg'
import iconFeature6 from '../assets/icon-feature-6.svg'
import iconFeature7 from '../assets/icon-feature-7.svg'

const navItemKeys = ['intro', 'features', 'requirements', 'download'] as const
const languageKeys = ['zhHans', 'zhHant', 'ja', 'ko', 'en'] as const
const activeLanguageIndex = ref(0)
const activeLanguageKey = computed(() => languageKeys[activeLanguageIndex.value])

const translations = {
  zhHans: {
    languageLabel: '简体中文',
    languageAria: '切换语言',
    nav: {
      intro: '简介',
      features: '特性',
      requirements: '需求',
      download: '下载',
    },
    topbarAria: '主导航',
    heroName: '小语文稿',
    tagline: ['言渡四海', '意会千山'],
    actions: {
      download: '应用下载',
      help: '帮助手册',
    },
    featureAria: '滚动切换特性文案',
    actionsAria: '主要操作',
    screenshotAlt: '小语文稿应用界面截图',
    features: [
      {
        title: '高性能 Markdown 编辑体验',
        description: '基于 GPU 渲染的 UI 与编辑器内核，跟随高刷新率屏幕',
      },
      {
        title: '高密度内容友好的排版',
        description: '代码式纯文本编辑体验，格式之上也能掌控每个字符',
      },
      {
        title: '扩展的 Markdown 语法支持',
        description: '文件属性，双向链接，表格编辑，上下标与脚注，公式与图表',
      },
      {
        title: '实时 Git 与 Diff 预览集成',
        description: '支持文件历史回滚，外部改动实时 Diff 与预览',
      },
      {
        title: '原生 AI Agent CLI 集成',
        description: '集成已安装的 Agent CLI 并 @ 内容到聊天',
      },
      {
        title: '完全无联网的本地文件管理',
        description: '支持代码文本编辑，图片预览，文件粘贴与拖拽',
      },
      {
        title: '集成手绘风格的绘图工具',
        description: '手绘风格绘图工具，连线文本与样式定制',
      },
    ],
  },
  zhHant: {
    languageLabel: '繁体中文',
    languageAria: '切換語言',
    nav: {
      intro: '簡介',
      features: '特性',
      requirements: '需求',
      download: '下載',
    },
    topbarAria: '主導覽',
    heroName: '小語文稿',
    tagline: ['言渡四海', '意會千山'],
    actions: {
      download: '應用下載',
      help: '幫助手冊',
    },
    featureAria: '捲動切換特性文案',
    actionsAria: '主要操作',
    screenshotAlt: '小語文稿應用程式介面截圖',
    features: [
      {
        title: '高效能 Markdown 編輯體驗',
        description: '基於 GPU 渲染的 UI 與編輯器核心，跟隨高更新率螢幕',
      },
      {
        title: '適合高密度內容的排版',
        description: '程式碼式純文字編輯體驗，在格式之上也能掌控每個字元',
      },
      {
        title: '擴充的 Markdown 語法支援',
        description: '檔案屬性、雙向連結、表格編輯、上下標與腳註、公式與圖表',
      },
      {
        title: '即時 Git 與 Diff 預覽整合',
        description: '支援檔案歷史回滾、外部改動即時 Diff 與預覽',
      },
      {
        title: '原生 AI Agent CLI 整合',
        description: '整合已安裝的 Agent CLI，並可將 @ 內容帶入聊天',
      },
      {
        title: '完全離線的本機檔案管理',
        description: '支援程式碼文字編輯、圖片預覽、檔案貼上與拖放',
      },
      {
        title: '整合手繪風格的繪圖工具',
        description: '手繪風格繪圖工具，支援連線文字與樣式自訂',
      },
    ],
  },
  ja: {
    languageLabel: '日本語',
    languageAria: '言語を切り替え',
    nav: {
      intro: '概要',
      features: '特長',
      requirements: '要件',
      download: '入手',
    },
    topbarAria: 'メインナビゲーション',
    heroName: '小語文稿',
    tagline: ['言葉は海を渡り', '想いは山を越える'],
    actions: {
      download: '入手',
      help: 'ガイド',
    },
    featureAria: 'スクロールで機能コピーを切り替え',
    actionsAria: '主な操作',
    screenshotAlt: '小語文稿アプリ画面のスクリーンショット',
    features: [
      {
        title: '高速 Markdown 編集',
        description: 'GPU UI と高速エディターでなめらか',
      },
      {
        title: '密な文書も読みやすく',
        description: 'プレーンテキストで書式を細かく制御',
      },
      {
        title: 'Markdown 拡張',
        description: '属性、リンク、表、脚注、数式、図表',
      },
      {
        title: 'Git / Diff プレビュー',
        description: '履歴復元と外部変更の差分を即確認',
      },
      {
        title: 'AI Agent CLI 連携',
        description: '導入済み CLI と @ 文脈をチャットへ',
      },
      {
        title: 'オフラインファイル管理',
        description: 'テキスト編集、画像表示、貼り付け、ドラッグ',
      },
      {
        title: '手描き作図ツール',
        description: '線の文字とスタイルを調整',
      },
    ],
  },
  ko: {
    languageLabel: '한국어',
    languageAria: '언어 전환',
    nav: {
      intro: '소개',
      features: '기능',
      requirements: '사양',
      download: '받기',
    },
    topbarAria: '주요 내비게이션',
    heroName: '소어문고',
    tagline: ['말은 바다를 건너고', '뜻은 산을 넘는다'],
    actions: {
      download: '앱 받기',
      help: '가이드',
    },
    featureAria: '스크롤로 기능 문구 전환',
    actionsAria: '주요 작업',
    screenshotAlt: '소어문고 앱 화면 스크린샷',
    features: [
      {
        title: '고속 Markdown 편집',
        description: 'GPU UI와 엔진으로 부드럽게 반응',
      },
      {
        title: '밀도 높은 문서 조판',
        description: '순수 텍스트로 서식을 세밀하게 제어',
      },
      {
        title: 'Markdown 확장 문법',
        description: '속성, 백링크, 표, 각주, 수식, 차트',
      },
      {
        title: 'Git / Diff 미리보기',
        description: '기록 복원과 외부 변경 Diff를 즉시 확인',
      },
      {
        title: 'AI Agent CLI 연동',
        description: '설치된 CLI와 @ 맥락을 채팅으로 전달',
      },
      {
        title: '오프라인 파일 관리',
        description: '텍스트 편집, 이미지 보기, 붙여넣기, 드래그',
      },
      {
        title: '손그림 다이어그램',
        description: '연결선 텍스트와 스타일 조정',
      },
    ],
  },
  en: {
    languageLabel: 'English',
    languageAria: 'Switch language',
    nav: {
      intro: 'Intro',
      features: 'Features',
      requirements: 'Needs',
      download: 'Download',
    },
    topbarAria: 'Primary navigation',
    heroName: 'Moeka Notes',
    tagline: ['Words cross seas', 'Meaning meets mountains'],
    actions: {
      download: 'Download',
      help: 'Guide',
    },
    featureAria: 'Scroll to switch feature copy',
    actionsAria: 'Primary actions',
    screenshotAlt: 'Moeka Notes app interface screenshot',
    features: [
      {
        title: 'Fast Markdown editing',
        description: 'GPU UI and editor core for smooth refresh',
      },
      {
        title: 'Dense writing layout',
        description: 'Plain text control down to each character',
      },
      {
        title: 'Extended Markdown',
        description: 'Properties, backlinks, tables, footnotes, formulas, charts',
      },
      {
        title: 'Git / Diff previews',
        description: 'Roll back history and inspect outside changes live',
      },
      {
        title: 'AI Agent CLI',
        description: 'Send installed CLI context into chat with @',
      },
      {
        title: 'Offline files',
        description: 'Edit text, preview images, paste and drag files',
      },
      {
        title: 'Hand-drawn diagrams',
        description: 'Tune connector text and styles',
      },
    ],
  },
}

const activeCopy = computed(() => translations[activeLanguageKey.value])
const navItems = computed(() => navItemKeys.map((key) => ({
  key,
  href: key === 'intro' ? '#简介' : key === 'features' ? '#特性' : `#${key}`,
  label: activeCopy.value.nav[key],
})))
const activeLanguage = computed(() => activeCopy.value.languageLabel)

const featureItems = [
  {
    icon: iconFeature1,
    copyIndex: 0,
  },
  {
    icon: iconFeature2,
    copyIndex: 1,
  },
  {
    icon: iconFeature3,
    copyIndex: 2,
  },
  {
    icon: iconFeature4,
    copyIndex: 3,
  },
  {
    icon: iconFeature5,
    copyIndex: 4,
  },
  {
    icon: iconFeature6,
    copyIndex: 5,
  },
  {
    icon: iconFeature7,
    copyIndex: 6,
  },
]

const activeFeatureIndex = ref(0)
const activeFeature = computed(() => featureItems[activeFeatureIndex.value])
const activeFeatureCopy = computed(() => activeCopy.value.features[activeFeature.value.copyIndex])
const featureTransitionDirection = ref<'next' | 'prev'>('next')
const featureTransitionName = computed(() => `feature-step-${featureTransitionDirection.value}`)
const isFeatureWheelGestureLocked = ref(false)
const featureWheelGestureDirection = ref<1 | -1 | null>(null)
const featureWheelUnlockTimer = ref<number | null>(null)
const FEATURE_WHEEL_GESTURE_IDLE_MS = 280

function showFeature(index: number) {
  const nextIndex = Math.min(featureItems.length - 1, Math.max(0, index))

  if (nextIndex === activeFeatureIndex.value) {
    return
  }

  featureTransitionDirection.value = nextIndex > activeFeatureIndex.value ? 'next' : 'prev'
  activeFeatureIndex.value = nextIndex
}

function handleFeatureWheel(event: WheelEvent) {
  if (event.deltaY === 0) {
    return
  }

  event.preventDefault()

  if (featureWheelUnlockTimer.value !== null) {
    window.clearTimeout(featureWheelUnlockTimer.value)
  }

  featureWheelUnlockTimer.value = window.setTimeout(() => {
    isFeatureWheelGestureLocked.value = false
    featureWheelGestureDirection.value = null
    featureWheelUnlockTimer.value = null
  }, FEATURE_WHEEL_GESTURE_IDLE_MS)

  const direction = event.deltaY > 0 ? 1 : -1

  if (isFeatureWheelGestureLocked.value && featureWheelGestureDirection.value === direction) {
    return
  }

  const nextIndex = activeFeatureIndex.value + direction

  if (nextIndex < 0 || nextIndex >= featureItems.length) {
    return
  }

  isFeatureWheelGestureLocked.value = true
  featureWheelGestureDirection.value = direction
  showFeature(nextIndex)
}

function handleFeatureKeydown(event: KeyboardEvent) {
  if (event.key === 'ArrowDown' || event.key === 'ArrowRight') {
    event.preventDefault()
    showFeature(activeFeatureIndex.value + 1)
  } else if (event.key === 'ArrowUp' || event.key === 'ArrowLeft') {
    event.preventDefault()
    showFeature(activeFeatureIndex.value - 1)
  }
}

function cycleLanguage() {
  activeLanguageIndex.value = (activeLanguageIndex.value + 1) % languageKeys.length
}
</script>

<template>
  <main
    class="site-shell"
    :data-language="activeLanguageKey"
    @wheel.capture="handleFeatureWheel"
  >
    <section class="hero" aria-labelledby="hero-title">
      <header class="topbar" :aria-label="activeCopy.topbarAria">
        <nav class="nav-pill">
          <a
            v-for="item in navItems"
            :key="item.key"
            :class="['nav-link', { active: item.key === 'intro' }]"
            :href="item.href"
          >
            {{ item.label }}
          </a>
        </nav>

        <button
          class="language-pill"
          type="button"
          :aria-label="activeCopy.languageAria"
          @click="cycleLanguage"
          @keydown.enter.prevent="cycleLanguage"
          @keydown.space.prevent="cycleLanguage"
        >
          <img src="../assets/icon-lang.svg" alt="" aria-hidden="true">
          {{ activeLanguage }}
        </button>
      </header>

      <div id="简介" class="hero-content">
        <img class="flower flower-left" src="../assets/flower-left.svg" alt="" aria-hidden="true">
        <img class="brand-logo" src="../assets/logo.svg" :alt="activeCopy.heroName" aria-describedby="hero-tagline">
        <img class="flower flower-right" src="../assets/flower-right.svg" alt="" aria-hidden="true">

        <h1 id="hero-title" class="sr-only">{{ activeCopy.heroName }}</h1>

        <p id="hero-tagline" class="tagline">
          <span>{{ activeCopy.tagline[0] }}</span>
          <span>{{ activeCopy.tagline[1] }}</span>
        </p>

        <div class="hero-actions" :aria-label="activeCopy.actionsAria">
          <a class="button button-dark" href="#下载">
            <img src="../assets/icon-download.svg" alt="" aria-hidden="true">
            {{ activeCopy.actions.download }}
          </a>
          <a class="button button-red" href="#手册">
            <img src="../assets/icon-help.svg" alt="" aria-hidden="true">
            {{ activeCopy.actions.help }}
          </a>
        </div>
      </div>
    </section>

    <section id="特性" class="feature-one" aria-labelledby="feature-title">
      <div
        class="feature-heading"
        tabindex="0"
        aria-live="polite"
        :aria-label="activeCopy.featureAria"
        @keydown="handleFeatureKeydown"
      >
        <Transition :name="featureTransitionName" mode="out-in">
          <div :key="activeFeature.copyIndex" class="feature-step">
            <img
              :src="activeFeature.icon"
              alt=""
              aria-hidden="true"
            >
            <div class="feature-copy">
              <h2 id="feature-title">{{ activeFeatureCopy.title }}</h2>
              <p>{{ activeFeatureCopy.description }}</p>
            </div>
          </div>
        </Transition>
      </div>

      <div class="product-screenshot-frame">
        <img class="product-screenshot" src="../assets/screenshot-feature-1.png" :alt="activeCopy.screenshotAlt">
        <svg
          class="version-badge"
          viewBox="0 0 120 120"
          role="img"
          aria-label="Ver 0.1 Beta"
        >
          <path
            class="version-badge-shadow"
            d="M65 5 C77 2 83 14 94 16 C108 19 106 35 116 43 C126 52 113 63 117 76 C121 91 104 94 99 107 C92 121 78 112 65 117 C51 123 44 110 31 108 C17 106 18 90 9 80 C0 69 10 58 6 46 C2 32 17 27 23 15 C31 1 49 12 65 5 Z"
          />
          <path
            class="version-badge-shape"
            d="M60 4 C72 -1 78 12 90 14 C104 16 103 32 113 40 C124 50 111 61 115 74 C120 89 103 93 98 105 C91 119 76 110 64 116 C50 123 43 110 30 107 C16 104 18 88 10 78 C1 67 9 56 5 44 C1 31 15 25 22 14 C31 0 47 12 60 4 Z"
          />
          <text class="version-badge-text" x="60" y="63">
            Ver 0.1 Beta
          </text>
        </svg>
      </div>
    </section>
  </main>
</template>
