import { defineConfig, presetIcons, transformerDirectives } from 'unocss';
import { presetWind4 } from 'unocss/preset-wind4';
import { presetAnimations } from 'unocss-preset-animations';
import { icons as phIcons } from '@iconify-json/ph';
import { bpThemeTokens } from '@tangle-network/blueprint-ui/preset';

export default defineConfig({
  content: {
    pipeline: {
      include: [
        /\.(tsx?|jsx?)$/,
        '../../blueprint-ui/src/**/*.{ts,tsx}',
        '../../ai-agent-sandbox-blueprint/packages/agent-ui/src/**/*.{ts,tsx}',
      ],
    },
  },
  transformers: [transformerDirectives()],
  presets: [
    presetWind4({
      dark: {
        light: '[data-theme="light"]',
        dark: '[data-theme="dark"]',
      },
    }),
    presetAnimations(),
    presetIcons({
      collections: {
        ph: () => phIcons,
      },
    }),
  ],
  rules: [
    [/^font-display$/, () => ({ 'font-family': "'Outfit', system-ui, sans-serif" })],
    [/^font-body$/, () => ({ 'font-family': "'DM Sans', system-ui, sans-serif" })],
    [/^font-data$/, () => ({ 'font-family': "'IBM Plex Mono', 'JetBrains Mono', monospace" })],
  ],
  theme: {
    colors: {
      bp: bpThemeTokens('claw'),
      claw: {
        elements: {
          borderColor: 'var(--claw-elements-borderColor)',
          borderColorActive: 'var(--claw-elements-borderColorActive)',
          background: {
            depth: {
              1: 'var(--claw-elements-bg-depth-1)',
              2: 'var(--claw-elements-bg-depth-2)',
              3: 'var(--claw-elements-bg-depth-3)',
              4: 'var(--claw-elements-bg-depth-4)',
            },
          },
          textPrimary: 'var(--claw-elements-textPrimary)',
          textSecondary: 'var(--claw-elements-textSecondary)',
          textTertiary: 'var(--claw-elements-textTertiary)',
          button: {
            primary: {
              background: 'var(--claw-elements-button-primary-background)',
              backgroundHover: 'var(--claw-elements-button-primary-backgroundHover)',
              text: 'var(--claw-elements-button-primary-text)',
            },
            secondary: {
              background: 'var(--claw-elements-button-secondary-background)',
              backgroundHover: 'var(--claw-elements-button-secondary-backgroundHover)',
              text: 'var(--claw-elements-button-secondary-text)',
            },
            danger: {
              background: 'var(--claw-elements-button-danger-background)',
              backgroundHover: 'var(--claw-elements-button-danger-backgroundHover)',
              text: 'var(--claw-elements-button-danger-text)',
            },
          },
          icon: {
            success: 'var(--claw-elements-icon-success)',
            error: 'var(--claw-elements-icon-error)',
            warning: 'var(--claw-elements-icon-warning)',
            primary: 'var(--claw-elements-icon-primary)',
            secondary: 'var(--claw-elements-icon-secondary)',
          },
          dividerColor: 'var(--claw-elements-dividerColor)',
          item: {
            backgroundHover: 'var(--claw-elements-item-backgroundHover)',
            backgroundActive: 'var(--claw-elements-item-backgroundActive)',
          },
          focus: 'var(--claw-elements-focus)',
        },
      },
    },
  },
  safelist: [
    'i-ph:arrow-clockwise',
    'i-ph:chat-circle',
    'i-ph:check-circle',
    'i-ph:copy',
    'i-ph:cube',
    'i-ph:hourglass',
    'i-ph:key',
    'i-ph:lock-key',
    'i-ph:play',
    'i-ph:robot',
    'i-ph:shield-check',
    'i-ph:terminal',
    'i-ph:warning-circle',
    'i-ph:x-circle',
  ],
});
