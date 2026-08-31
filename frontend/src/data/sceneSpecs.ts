import type { JobPhase } from '../types/job'

export type JiangnanSceneId = 0 | 1 | 2 | 3 | 4 | 5

export interface SceneCamera {
  zoom: number
  offset: readonly [number, number]
}

export interface JiangnanSceneSpec {
  id: JiangnanSceneId
  desktopSrc: string
  mobileSrc: string
  desktopSize: readonly [number, number]
  mobileSize: readonly [number, number]
  desktopCamera: SceneCamera
  mobileCamera: SceneCamera
  desktopLensOrigin: readonly [number, number]
  mobileLensOrigin: readonly [number, number]
  rain: number
}

const ASSET_ROOT = '/assets/jiangnan'

// Camera and lens values preserve the hand-directed framing from the approved v10 prototype.
export const JIANGNAN_SCENES: readonly JiangnanSceneSpec[] = [
  {
    id: 0,
    desktopSrc: `${ASSET_ROOT}/scene-00-window.webp`,
    mobileSrc: `${ASSET_ROOT}/scene-00-window-mobile.webp`,
    desktopSize: [1672, 941],
    mobileSize: [941, 1672],
    desktopCamera: { zoom: 1.05, offset: [0.015, 0.006] },
    mobileCamera: { zoom: 1.035, offset: [0, 0.004] },
    desktopLensOrigin: [0.57, 0.54],
    mobileLensOrigin: [0.52, 0.57],
    rain: 0.42,
  },
  {
    id: 1,
    desktopSrc: `${ASSET_ROOT}/scene-01-eaves.webp`,
    mobileSrc: `${ASSET_ROOT}/scene-01-eaves-mobile.webp`,
    desktopSize: [1672, 941],
    mobileSize: [941, 1672],
    desktopCamera: { zoom: 1.045, offset: [-0.012, 0.002] },
    mobileCamera: { zoom: 1.04, offset: [0.006, 0] },
    desktopLensOrigin: [0.31, 0.37],
    mobileLensOrigin: [0.46, 0.38],
    rain: 0.96,
  },
  {
    id: 2,
    desktopSrc: `${ASSET_ROOT}/scene-02-bridge.webp`,
    mobileSrc: `${ASSET_ROOT}/scene-02-bridge-mobile.webp`,
    desktopSize: [1672, 941],
    mobileSize: [941, 1672],
    desktopCamera: { zoom: 1.055, offset: [0.01, 0] },
    mobileCamera: { zoom: 1.045, offset: [-0.005, 0] },
    desktopLensOrigin: [0.73, 0.48],
    mobileLensOrigin: [0.64, 0.46],
    rain: 0.78,
  },
  {
    id: 3,
    desktopSrc: `${ASSET_ROOT}/scene-03-boat.webp`,
    mobileSrc: `${ASSET_ROOT}/scene-03-boat-mobile.webp`,
    desktopSize: [1672, 941],
    mobileSize: [941, 1672],
    desktopCamera: { zoom: 1.04, offset: [-0.016, -0.004] },
    mobileCamera: { zoom: 1.035, offset: [0.004, -0.004] },
    desktopLensOrigin: [0.43, 0.33],
    mobileLensOrigin: [0.48, 0.35],
    rain: 0.86,
  },
  {
    id: 4,
    desktopSrc: `${ASSET_ROOT}/scene-04-table.webp`,
    mobileSrc: `${ASSET_ROOT}/scene-04-table-mobile.webp`,
    desktopSize: [1672, 941],
    mobileSize: [941, 1672],
    desktopCamera: { zoom: 1.052, offset: [0.008, 0.008] },
    mobileCamera: { zoom: 1.04, offset: [0, 0.006] },
    desktopLensOrigin: [0.66, 0.49],
    mobileLensOrigin: [0.55, 0.45],
    rain: 0.5,
  },
  {
    id: 5,
    desktopSrc: `${ASSET_ROOT}/scene-05-dawn.webp`,
    mobileSrc: `${ASSET_ROOT}/scene-05-dawn-mobile.webp`,
    desktopSize: [1672, 941],
    mobileSize: [941, 1672],
    desktopCamera: { zoom: 1.035, offset: [0, -0.005] },
    mobileCamera: { zoom: 1.025, offset: [0, -0.004] },
    desktopLensOrigin: [0.5, 0.5],
    mobileLensOrigin: [0.5, 0.5],
    rain: 0.24,
  },
]

// Login-sensitive recovery states never imply a valid QQ session when the backend says otherwise.
export function sceneForPhase(
  phase: JobPhase | null | undefined,
  loggedIn = false,
): JiangnanSceneId {
  switch (phase) {
    case undefined:
    case null:
      return 0
    case 'awaitingLogin':
    case 'failed':
    case 'interrupted':
      return 1
    case 'loggedIn':
      return 2
    case 'paused':
    case 'cancelled':
      return loggedIn ? 2 : 1
    case 'queued':
    case 'archiving':
      return 3
    case 'downloadingMedia':
    case 'packaging':
      return 4
    case 'ready':
      return 5
  }
}
