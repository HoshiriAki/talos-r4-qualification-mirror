import { definePreset } from '@primevue/themes'
import Aura from '@primevue/themes/aura'

// Terminal Brutalist custom preset for PrimeVue styled mode (Aura).
// Noir monochrome primary, sharp borders, Space Mono typography.
// darkModeSelector matches the project's data-theme attribute.

const TerminalBrutalist = definePreset(Aura, {
  primitive: {
    borderRadius: {
      none: '0',
      xs: '4px',
      sm: '4px',
      md: '8px',
      lg: '10px',
      xl: '12px',
    },
  },
  semantic: {
    transitionDuration: '0.15s',
    focusRing: {
      width: '1px',
      style: 'solid',
      color: '{primary.color}',
      offset: '2px',
      shadow: 'none',
    },
    primary: {
      50: '{zinc.50}',
      100: '{zinc.100}',
      200: '{zinc.200}',
      300: '{zinc.300}',
      400: '{zinc.400}',
      500: '{zinc.500}',
      600: '{zinc.600}',
      700: '{zinc.700}',
      800: '{zinc.800}',
      900: '{zinc.900}',
      950: '{zinc.950}',
    },
    colorScheme: {
      light: {
        primary: {
          color: '{zinc.900}',
          contrastColor: '#ffffff',
          hoverColor: '{zinc.800}',
          activeColor: '{zinc.700}',
        },
        highlight: {
          background: '{zinc.100}',
          focusBackground: '{zinc.200}',
          color: '{zinc.900}',
          focusColor: '{zinc.900}',
        },
        surface: {
          0: '#ffffff',
          50: '{slate.50}',
          100: '{slate.100}',
          200: '{slate.200}',
          300: '{slate.300}',
          400: '{slate.400}',
          500: '{slate.500}',
          600: '{slate.600}',
          700: '{slate.700}',
          800: '{slate.800}',
          900: '{slate.900}',
          950: '{slate.950}',
        },
        formField: {
          background: '#ffffff',
          disabledBackground: '{surface.100}',
          filledBackground: '{surface.50}',
          filledHoverBackground: '{surface.50}',
          filledFocusBackground: '{surface.50}',
          borderColor: '{surface.300}',
          hoverBorderColor: '{surface.500}',
          focusBorderColor: '{primary.color}',
          invalidBorderColor: '{red.400}',
          color: '{surface.900}',
          disabledColor: '{surface.400}',
          placeholderColor: '{surface.400}',
          floatLabelColor: '{surface.500}',
          floatLabelFocusColor: '{primary.color}',
          floatLabelActiveColor: '{surface.500}',
          iconColor: '{surface.400}',
          shadow: 'none',
          borderRadius: '8px',
        },
        content: {
          background: '#ffffff',
          hoverBackground: '{surface.100}',
          borderColor: '{surface.200}',
          color: '{text.color}',
          hoverColor: '{text.hover.color}',
        },
        text: {
          color: '{surface.900}',
          hoverColor: '{surface.800}',
          mutedColor: '{surface.500}',
          hoverMutedColor: '{surface.600}',
        },
        navigation: {
          item: {
            focusBackground: '{surface.100}',
            activeBackground: '{surface.100}',
            color: '{text.color}',
            focusColor: '{text.hover.color}',
            activeColor: '{text.hover.color}',
            icon: {
              color: '{surface.400}',
              focusColor: '{surface.500}',
              activeColor: '{surface.500}',
            },
          },
        },
      },
      dark: {
        primary: {
          color: '{zinc.300}',
          contrastColor: '{zinc.900}',
          hoverColor: '{zinc.200}',
          activeColor: '{zinc.100}',
        },
        highlight: {
          background: 'color-mix(in srgb, {primary.color}, transparent 84%)',
          focusBackground: 'color-mix(in srgb, {primary.color}, transparent 76%)',
          color: 'rgba(255,255,255,.87)',
          focusColor: 'rgba(255,255,255,.87)',
        },
        surface: {
          0: '#ffffff',
          50: '{zinc.50}',
          100: '{zinc.100}',
          200: '{zinc.200}',
          300: '{zinc.300}',
          400: '{zinc.400}',
          500: '{zinc.500}',
          600: '{zinc.600}',
          700: '{zinc.700}',
          800: '{zinc.800}',
          900: '{zinc.900}',
          950: '{zinc.950}',
        },
        formField: {
          background: '#1A1A1C',
          disabledBackground: '{surface.700}',
          filledBackground: '{surface.800}',
          filledHoverBackground: '{surface.800}',
          filledFocusBackground: '{surface.800}',
          borderColor: '#3E3E44',
          hoverBorderColor: '#5C5C64',
          focusBorderColor: '#D4D4D8',
          invalidBorderColor: '{red.300}',
          color: '{surface.0}',
          disabledColor: '{surface.400}',
          placeholderColor: '#64646C',
          floatLabelColor: '{surface.400}',
          floatLabelFocusColor: '{surface.0}',
          floatLabelActiveColor: '{surface.400}',
          iconColor: '{surface.400}',
          shadow: 'none',
          borderRadius: '8px',
        },
        content: {
          background: '#1E1E20',
          hoverBackground: '#28282C',
          borderColor: '#3E3E44',
          color: '{text.color}',
          hoverColor: '{text.hover.color}',
        },
        text: {
          color: '#D4D4D8',
          hoverColor: '{surface.0}',
          mutedColor: '#64646C',
          hoverMutedColor: '#94949C',
        },
        navigation: {
          item: {
            focusBackground: '#28282C',
            activeBackground: '#28282C',
            color: '{text.color}',
            focusColor: '{text.hover.color}',
            activeColor: '{text.hover.color}',
            icon: {
              color: '{surface.500}',
              focusColor: '{surface.400}',
              activeColor: '{surface.400}',
            },
          },
        },
        overlay: {
          select: {
            background: '#1E1E20',
            borderColor: '#3E3E44',
            color: '{text.color}',
          },
          popover: {
            background: '#1E1E20',
            borderColor: '#3E3E44',
            color: '{text.color}',
          },
          modal: {
            background: '#1E1E20',
            borderColor: '#3E3E44',
            color: '{text.color}',
          },
        },
        list: {
          option: {
            focusBackground: '#28282C',
            selectedBackground: '{highlight.background}',
            selectedFocusBackground: '{highlight.focus.background}',
            color: '{text.color}',
            focusColor: '{text.hover.color}',
            selectedColor: '{highlight.color}',
            selectedFocusColor: '{highlight.focus.color}',
            icon: {
              color: '{surface.500}',
              focusColor: '{surface.400}',
            },
          },
        },
      },
    },
  },

  // ═══ COMPONENT TOKENS ═══

  components: {
    // ── Button ──
    button: {
      root: {
        borderRadius: '10px',
        paddingX: '1rem',
        paddingY: '0.5rem',
        gap: '0.5rem',
        fontSize: '0.75rem',
        fontWeight: '400',
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
        letterSpacing: '0.05em',
        textTransform: 'uppercase',
        transitionDuration: '0.15s',
      },
      colorScheme: {
        light: {
          root: {
            primary: {
              background: '{surface.900}',
              hoverBackground: '{surface.800}',
              activeBackground: '{surface.700}',
              color: '{surface.0}',
              borderColor: '{surface.900}',
            },
            secondary: {
              background: '{surface.100}',
              hoverBackground: '{surface.200}',
              activeBackground: '{surface.300}',
              color: '{surface.600}',
              borderColor: '{surface.300}',
            },
            danger: {
              background: '#FDE8E8',
              hoverBackground: '#FBC8C8',
              activeBackground: '#F8A8A8',
              color: '#C04040',
              borderColor: '#FBC8C8',
            },
            info: {
              background: 'transparent',
              hoverBackground: '{surface.100}',
              activeBackground: '{surface.200}',
              color: '{text.muted.color}',
              borderColor: 'transparent',
            },
          },
          text: {
            primary: {
              background: 'transparent',
              hoverBackground: '{surface.100}',
              activeBackground: '{surface.200}',
              color: '{text.muted.color}',
              borderColor: 'transparent',
            },
          },
        },
        dark: {
          root: {
            primary: {
              background: '{surface.300}',
              hoverBackground: '{surface.200}',
              activeBackground: '{surface.100}',
              color: '{surface.900}',
              borderColor: '{surface.300}',
            },
            secondary: {
              background: '#28282C',
              hoverBackground: '#323236',
              activeBackground: '#3E3E44',
              color: '#B0B0B8',
              borderColor: '#3E3E44',
            },
            danger: {
              background: '#2C1E1E',
              hoverBackground: '#3A2626',
              activeBackground: '#4A3030',
              color: '#C88080',
              borderColor: '#4A3030',
            },
            info: {
              background: 'transparent',
              hoverBackground: '#28282C',
              activeBackground: '#323236',
              color: '{text.muted.color}',
              borderColor: 'transparent',
            },
          },
          text: {
            primary: {
              background: 'transparent',
              hoverBackground: '#28282C',
              activeBackground: '#323236',
              color: '{text.muted.color}',
              borderColor: 'transparent',
            },
          },
        },
      },
    },

    // ── InputText ──
    inputtext: {
      root: {
        borderRadius: '8px',
        paddingX: '0.75rem',
        paddingY: '0.5rem',
        fontSize: '0.8125rem',
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
        background: '{formField.background}',
        borderColor: '{formField.borderColor}',
        hoverBorderColor: '{formField.hoverBorderColor}',
        focusBorderColor: '{formField.focusBorderColor}',
        color: '{formField.color}',
        placeholderColor: '{formField.placeholder.color}',
        shadow: 'none',
      },
    },

    // ── InputNumber ──
    inputnumber: {
      root: {
        borderRadius: '8px',
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
      },
      input: {
        fontSize: '0.8125rem',
        paddingX: '0.75rem',
        paddingY: '0.5rem',
        background: '{formField.background}',
        borderColor: '{formField.borderColor}',
        hoverBorderColor: '{formField.hoverBorderColor}',
        focusBorderColor: '{formField.focusBorderColor}',
        color: '{formField.color}',
      },
      button: {
        background: 'transparent',
        borderColor: '{formField.borderColor}',
        color: '{text.muted.color}',
        hoverBackground: '{content.hover.background}',
        hoverColor: '{text.color}',
        width: '1.5rem',
        borderRadius: '0',
      },
    },

    // ── Select ──
    select: {
      root: {
        borderRadius: '8px',
        fontSize: '0.8125rem',
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
        background: '{formField.background}',
        borderColor: '{formField.borderColor}',
        hoverBorderColor: '{formField.hoverBorderColor}',
        focusBorderColor: '{formField.focusBorderColor}',
        color: '{formField.color}',
        shadow: 'none',
      },
      overlay: {
        borderRadius: '8px',
        background: '{content.background}',
        borderColor: '{content.borderColor}',
      },
      option: {
        fontSize: '0.8125rem',
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
        color: '{text.color}',
        focusBackground: '{content.hover.background}',
        focusColor: '{text.hover.color}',
        selectedBackground: '{highlight.background}',
        selectedColor: '{highlight.color}',
      },
      colorScheme: {
        dark: {
          overlay: {
            shadow: '0 4px 12px rgba(0,0,0,0.3)',
          },
        },
      },
    },

    // ── Textarea ──
    textarea: {
      root: {
        borderRadius: '8px',
        paddingX: '0.75rem',
        paddingY: '0.5rem',
        fontSize: '0.8125rem',
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
        background: '{formField.background}',
        borderColor: '{formField.borderColor}',
        hoverBorderColor: '{formField.hoverBorderColor}',
        focusBorderColor: '{formField.focusBorderColor}',
        color: '{formField.color}',
      },
    },

    // ── Dialog ──
    dialog: {
      root: {
        borderRadius: '12px',
        background: '{content.background}',
        borderColor: '{content.borderColor}',
        color: '{text.color}',
        shadow: '0 4px 24px rgba(0,0,0,0.4)',
      },
      header: {
        padding: '1rem 1.5rem',
        gap: '0.75rem',
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
        fontSize: '1rem',
        fontWeight: '700',
        letterSpacing: '0.05em',
        color: '{text.color}',
        borderColor: '{content.borderColor}',
        borderWidth: '0 0 1px 0',
      },
      content: {
        padding: '1.5rem',
        fontSize: '0.8125rem',
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
      },
      footer: {
        padding: '1rem 1.5rem',
        gap: '0.5rem',
        borderColor: '{content.borderColor}',
        borderWidth: '1px 0 0 0',
      },
      closeButton: {
        borderRadius: '4px',
        color: '{text.muted.color}',
        hoverBackground: '{content.hover.background}',
        hoverColor: '{text.color}',
      },
    },

    // ── Tag (badge replacement) ──
    tag: {
      root: {
        borderRadius: '10px',
        paddingX: '0.5rem',
        paddingY: '0.125rem',
        fontSize: '0.6875rem',
        fontWeight: '400',
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
        gap: '0.25rem',
      },
    },

    // ── DatePicker ──
    datepicker: {
      root: {
        borderRadius: '8px',
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
      },
      input: {
        fontSize: '0.8125rem',
        paddingX: '0.75rem',
        paddingY: '0.5rem',
        background: '{formField.background}',
        borderColor: '{formField.borderColor}',
        hoverBorderColor: '{formField.hoverBorderColor}',
        focusBorderColor: '{formField.focusBorderColor}',
        color: '{formField.color}',
        shadow: 'none',
        borderRadius: '8px',
      },
      panel: {
        background: '{content.background}',
        borderColor: '{content.borderColor}',
        borderRadius: '8px',
        shadow: '0 4px 12px rgba(0,0,0,0.3)',
      },
      header: {
        background: '{content.background}',
        borderColor: '{content.borderColor}',
        color: '{text.color}',
        padding: '0.5rem',
      },
      title: {
        fontSize: '0.75rem',
        fontWeight: '700',
        letterSpacing: '0.05em',
      },
      monthYear: {
        fontSize: '0.75rem',
        fontWeight: '700',
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
      },
      dayLabel: {
        color: '{text.muted.color}',
        fontSize: '0.6875rem',
        fontWeight: '400',
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
        textTransform: 'uppercase',
        letterSpacing: '0.05em',
      },
      dayCell: {
        borderRadius: '4px',
        fontSize: '0.75rem',
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
        hoverBackground: '{content.hover.background}',
        selectedBackground: '{primary.color}',
        selectedColor: '{primary.contrast.color}',
        rangeBackground: '{highlight.background}',
        todayBorderColor: '{primary.color}',
      },
      navButton: {
        borderRadius: '4px',
        color: '{text.muted.color}',
        hoverBackground: '{content.hover.background}',
        hoverColor: '{text.color}',
      },
    },

    // ── FloatLabel ──
    floatlabel: {
      root: {
        fontFamily: "'Space Mono', 'Courier New', 'Microsoft YaHei', monospace",
        fontSize: '0.75rem',
        color: '{formField.floatLabel.color}',
        focusColor: '{formField.floatLabel.focus.color}',
        activeColor: '{formField.floatLabel.active.color}',
        invalidColor: '{formField.invalid.border.color}',
      },
    },

    // ── DataTable ──
    datatable: {
      colorScheme: {
        light: {
          root: { borderColor: '{surface.200}' },
          header: { background: '{surface.100}', borderColor: '{surface.200}', color: '{text.muted.color}' },
          headerCell: {
            background: '{surface.100}', hoverBackground: '{surface.200}',
            borderColor: '{surface.200}', color: '{text.muted.color}',
            focusRing: { offset: '-1px' },
          },
          bodyCell: { borderColor: '{surface.100}', selectedBorderColor: '{primary.200}' },
          row: { stripedBackground: '{surface.50}' },
          sortIcon: { color: '{surface.400}', hoverColor: '{surface.600}' },
          rowToggleButton: {
            hoverBackground: '{surface.100}', selectedHoverBackground: '{surface.0}',
            color: '{text.muted.color}', hoverColor: '{text.color}', selectedHoverColor: '{primary.color}',
          },
          paginatorTop: { borderColor: '{surface.200}' },
          paginatorBottom: { borderColor: '{surface.200}' },
        },
        dark: {
          root: { borderColor: '#3E3E44' },
          header: { background: '#242428', borderColor: '#3E3E44', color: '#94949C' },
          headerCell: {
            background: '#242428', hoverBackground: '#28282C',
            borderColor: '#3E3E44', color: '#94949C',
            focusRing: { offset: '-1px' },
          },
          bodyCell: { borderColor: '#3E3E44', selectedBorderColor: '{zinc.700}' },
          row: { stripedBackground: '#1E1E20' },
          sortIcon: { color: '#64646C', hoverColor: '#94949C' },
          rowToggleButton: {
            hoverBackground: '#28282C', selectedHoverBackground: '#1E1E20',
            color: '#64646C', hoverColor: '#D4D4D8', selectedHoverColor: '{primary.color}',
          },
          paginatorTop: { borderColor: '#3E3E44' },
          paginatorBottom: { borderColor: '#3E3E44' },
        },
      },
    },

    // ── Paginator ──
    paginator: {
      colorScheme: {
        light: {
          root: { background: '{surface.100}', color: '{text.color}', borderRadius: '0' },
          navButton: {
            background: 'transparent', hoverBackground: '{surface.200}',
            selectedBackground: '{highlight.background}', color: '{text.muted.color}',
            hoverColor: '{text.color}', selectedColor: '{highlight.color}', borderRadius: '4px',
          },
        },
        dark: {
          root: { background: '#242428', color: '{text.color}', borderRadius: '0' },
          navButton: {
            background: 'transparent', hoverBackground: '#28282C',
            selectedBackground: '{highlight.background}', color: '#94949C',
            hoverColor: '#D4D4D8', selectedColor: '{highlight.color}', borderRadius: '4px',
          },
        },
      },
    },

    // ── Toast ──
    toast: {
      colorScheme: {
        light: { root: { borderRadius: '4px' } },
        dark: { root: { borderRadius: '4px' } },
      },
    },
  },
})

export default TerminalBrutalist
