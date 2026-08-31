import { onMounted, onScopeDispose, readonly, shallowRef, watch, type Ref } from 'vue'
import { JIANGNAN_SCENES, type JiangnanSceneId } from '../data/sceneSpecs'

type MotionPolicy = 'full' | 'lite' | 'off'
type UniformName =
  | 'uFrom'
  | 'uTo'
  | 'uResolution'
  | 'uTextureSize'
  | 'uPointer'
  | 'uFromOffset'
  | 'uToOffset'
  | 'uDropCenter'
  | 'uFromZoom'
  | 'uToZoom'
  | 'uTransition'
  | 'uTime'
  | 'uRain'
  | 'uMotion'
  | 'uArchiveProgress'

export interface SceneFxOptions {
  host: Ref<HTMLElement | null>
  canvas: Ref<HTMLCanvasElement | null>
  scene: Readonly<Ref<JiangnanSceneId>>
  progress: Readonly<Ref<number>>
}

const VERTEX_SHADER = `#version 300 es
in vec2 aPosition;
out vec2 vUv;

void main() {
  vUv = aPosition * 0.5 + 0.5;
  gl_Position = vec4(aPosition, 0.0, 1.0);
}`

const FRAGMENT_SHADER = `#version 300 es
precision highp float;

in vec2 vUv;
out vec4 fragColor;

uniform sampler2D uFrom;
uniform sampler2D uTo;
uniform vec2 uResolution;
uniform vec2 uTextureSize;
uniform vec2 uPointer;
uniform vec2 uFromOffset;
uniform vec2 uToOffset;
uniform vec2 uDropCenter;
uniform float uFromZoom;
uniform float uToZoom;
uniform float uTransition;
uniform float uTime;
uniform float uRain;
uniform float uMotion;
uniform float uArchiveProgress;

float hash21(vec2 point) {
  point = fract(point * vec2(123.34, 456.21));
  point += dot(point, point + 45.32);
  return fract(point.x * point.y);
}

vec2 coverUv(vec2 uv, float zoom, vec2 offset) {
  float canvasAspect = uResolution.x / uResolution.y;
  float imageAspect = uTextureSize.x / uTextureSize.y;
  vec2 visible = vec2(1.0);

  if (imageAspect > canvasAspect) {
    visible.x = canvasAspect / imageAspect;
  } else {
    visible.y = imageAspect / canvasAspect;
  }

  vec2 cameraUv = (uv - 0.5) / zoom + 0.5 + offset;
  return (cameraUv - 0.5) * visible + 0.5;
}

vec3 sampleScene(sampler2D image, vec2 uv, float zoom, vec2 offset) {
  return texture(image, coverUv(uv, zoom, offset)).rgb;
}

float rainLayer(vec2 uv, float columns, float speed, float seed) {
  vec2 grid = uv * vec2(columns, 7.0);
  grid.y += uTime * speed;
  vec2 cell = floor(grid);
  vec2 local = fract(grid);
  float random = hash21(cell + seed);
  float x = abs(local.x - mix(0.18, 0.82, random));
  float stroke = 1.0 - smoothstep(0.006, 0.03, x);
  float segment = smoothstep(0.03, 0.18, local.y) * (1.0 - smoothstep(0.48, 0.98, local.y));
  float exists = step(0.72, hash21(vec2(cell.x, cell.y * 0.23 + seed * 7.0)));
  return stroke * segment * exists;
}

void main() {
  float aspect = uResolution.x / uResolution.y;
  vec2 parallax = uPointer * vec2(0.006, 0.004) * uMotion;
  vec3 fromColor = sampleScene(uFrom, vUv, uFromZoom, uFromOffset + parallax);

  float eased = uTransition * uTransition * (3.0 - 2.0 * uTransition);
  vec2 aspectScale = vec2(aspect, 1.0);
  vec2 dropVector = (vUv - uDropCenter) * aspectScale;
  float distanceToDrop = length(dropVector);
  float radius = mix(0.018, 1.58, pow(eased, 1.42));
  float activeTransition = smoothstep(0.0, 0.035, uTransition);
  float edgeAngle = atan(dropVector.y, dropVector.x);
  float edgeWave = (
    sin(edgeAngle * 3.0 + uTime * 0.42) * 0.008 +
    sin(edgeAngle * 7.0 - uTime * 0.31) * 0.0035
  ) * (1.0 - eased * 0.76) * uMotion;
  float aperture = (1.0 - smoothstep(radius - 0.02, radius + 0.026, distanceToDrop + edgeWave)) * activeTransition;

  vec2 dropNormal = normalize(dropVector + vec2(0.0001)) / aspectScale;
  float inside = 1.0 - smoothstep(0.0, max(radius, 0.001), distanceToDrop);
  float refraction = sin((radius - distanceToDrop) * 28.0) * 0.0075 * inside * (1.0 - eased * 0.6) * uMotion;
  vec2 refractedUv = vUv + dropNormal * refraction;

  vec2 toOffset = uToOffset - parallax * 0.6;
  vec3 toColor;
  toColor.r = sampleScene(uTo, refractedUv + dropNormal * 0.0011, uToZoom, toOffset).r;
  toColor.g = sampleScene(uTo, refractedUv, uToZoom, toOffset).g;
  toColor.b = sampleScene(uTo, refractedUv - dropNormal * 0.0008, uToZoom, toOffset).b;

  vec3 color = mix(fromColor, toColor, aperture);
  float rim = exp(-abs(distanceToDrop - radius) * 72.0) * activeTransition * (1.0 - eased * 0.58);
  float innerRim = exp(-abs(distanceToDrop - radius + 0.022) * 95.0) * activeTransition;
  color = mix(color, vec3(0.9, 0.91, 0.88), rim * 0.48 * uMotion);
  color *= 1.0 - innerRim * 0.085 * uMotion;

  float rain = rainLayer(vUv + vec2(0.0, uPointer.y * 0.005), 42.0, 0.92, 1.7);
  rain += rainLayer(vUv * 1.13, 29.0, 0.67, 8.2) * 0.65;
  rain += rainLayer(vUv * 0.87, 18.0, 0.48, 16.4) * 0.42;
  color += vec3(0.75, 0.77, 0.75) * rain * 0.075 * uRain * uMotion;

  float waterBand = smoothstep(0.0, 0.36, 1.0 - vUv.y);
  float recoveryRipple = sin((vUv.x + vUv.y * 0.23) * 54.0 - uTime * 1.2) * 0.5 + 0.5;
  color += vec3(0.34, 0.25, 0.17) * recoveryRipple * waterBand * uArchiveProgress * 0.012;

  float grain = hash21(vUv * uResolution + fract(uTime) * 91.7) - 0.5;
  color += grain * 0.018 * uMotion;
  fragColor = vec4(color, 1.0);
}`

const UNIFORM_NAMES: readonly UniformName[] = [
  'uFrom',
  'uTo',
  'uResolution',
  'uTextureSize',
  'uPointer',
  'uFromOffset',
  'uToOffset',
  'uDropCenter',
  'uFromZoom',
  'uToZoom',
  'uTransition',
  'uTime',
  'uRain',
  'uMotion',
  'uArchiveProgress',
]

const clamp = (value: number, minimum = 0, maximum = 1) =>
  Math.min(maximum, Math.max(minimum, value))

const lerp = (from: number, to: number, amount: number) => from + (to - from) * amount

function createShader(gl: WebGL2RenderingContext, type: number, source: string): WebGLShader {
  const shader = gl.createShader(type)
  if (!shader) throw new Error('WebGL shader allocation failed')

  gl.shaderSource(shader, source)
  gl.compileShader(shader)
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const message = gl.getShaderInfoLog(shader) || 'WebGL shader compilation failed'
    gl.deleteShader(shader)
    throw new Error(message)
  }
  return shader
}

function createProgram(gl: WebGL2RenderingContext): WebGLProgram {
  const vertexShader = createShader(gl, gl.VERTEX_SHADER, VERTEX_SHADER)
  let fragmentShader: WebGLShader
  try {
    fragmentShader = createShader(gl, gl.FRAGMENT_SHADER, FRAGMENT_SHADER)
  } catch (error) {
    gl.deleteShader(vertexShader)
    throw error
  }
  const program = gl.createProgram()

  if (!program) {
    gl.deleteShader(vertexShader)
    gl.deleteShader(fragmentShader)
    throw new Error('WebGL program allocation failed')
  }

  gl.attachShader(program, vertexShader)
  gl.attachShader(program, fragmentShader)
  gl.linkProgram(program)
  gl.deleteShader(vertexShader)
  gl.deleteShader(fragmentShader)

  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const message = gl.getProgramInfoLog(program) || 'WebGL program linking failed'
    gl.deleteProgram(program)
    throw new Error(message)
  }
  return program
}

function loadImage(source: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image()
    image.decoding = 'async'
    image.fetchPriority = 'low'
    image.onload = () => {
      image.onload = null
      image.onerror = null
      resolve(image)
    }
    image.onerror = () => {
      image.onload = null
      image.onerror = null
      reject(new Error(`Unable to read local scene asset: ${source}`))
    }
    image.src = source
  })
}

/** Owns the single animation scheduler for rain, parallax, progress ripples and lens transitions. */
class SceneFxRenderer {
  private gl: WebGL2RenderingContext | null = null
  private program: WebGLProgram | null = null
  private positionBuffer: WebGLBuffer | null = null
  private vertexArray: WebGLVertexArrayObject | null = null
  private uniforms = new Map<UniformName, WebGLUniformLocation | null>()
  private textures: WebGLTexture[] = []
  private images: HTMLImageElement[] = []
  private textureSize: readonly [number, number] = [1672, 941]
  private mobile = false
  private fromScene: JiangnanSceneId
  private toScene: JiangnanSceneId
  private transitionStartedAt = 0
  private transition = 0
  private progress = 0
  private pointer = { x: 0, y: 0, targetX: 0, targetY: 0 }
  private documentVisible = !document.hidden
  private stageVisible = true
  private frameId = 0
  private lastDrawAt = 0
  private clockOrigin = performance.now()
  private loadGeneration = 0
  private destroyed = false
  private ready = false
  private readonly reducedMotionQuery: MediaQueryList | null
  private readonly mobileQuery: MediaQueryList | null
  private readonly compactQuery: MediaQueryList | null
  private readonly saveData: boolean
  private resizeObserver: ResizeObserver | null = null
  private intersectionObserver: IntersectionObserver | null = null

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly host: HTMLElement,
    initialScene: JiangnanSceneId,
    initialProgress: number,
    private readonly onReadyChange: (ready: boolean) => void,
  ) {
    this.fromScene = initialScene
    this.toScene = initialScene
    this.progress = clamp(initialProgress)
    this.reducedMotionQuery = this.queryMedia('(prefers-reduced-motion: reduce)')
    this.mobileQuery = this.queryMedia('(max-width: 720px)')
    this.compactQuery = this.queryMedia('(max-width: 720px), (max-height: 690px)')
    this.saveData = Boolean(
      (navigator as Navigator & { connection?: { saveData?: boolean } }).connection?.saveData,
    )
    this.mobile = this.isMobileViewport

    if (typeof WebGL2RenderingContext !== 'undefined') {
      try {
        this.gl = canvas.getContext('webgl2', {
          alpha: false,
          antialias: false,
          depth: false,
          powerPreference: 'high-performance',
        })
        if (this.gl) this.setupWebGl()
      } catch {
        this.releaseWebGlResources()
        this.gl = null
      }
    }

    this.bindEvents()
    this.observeHost()
    this.resize()
    if (this.gl) void this.loadAssets()
  }

  private queryMedia(query: string): MediaQueryList | null {
    return typeof window.matchMedia === 'function' ? window.matchMedia(query) : null
  }

  private get isMobileViewport(): boolean {
    return this.mobileQuery?.matches ?? window.innerWidth <= 720
  }

  private get motionPolicy(): MotionPolicy {
    if (this.reducedMotionQuery?.matches) return 'off'
    const compact =
      this.compactQuery?.matches ?? (window.innerWidth <= 720 || window.innerHeight <= 690)
    if (this.saveData || compact) return 'lite'
    return 'full'
  }

  private setupWebGl(): void {
    const gl = this.gl
    if (!gl) return

    this.program = createProgram(gl)
    this.positionBuffer = gl.createBuffer()
    this.vertexArray = gl.createVertexArray()
    if (!this.positionBuffer || !this.vertexArray) throw new Error('WebGL buffer allocation failed')

    gl.bindVertexArray(this.vertexArray)
    gl.bindBuffer(gl.ARRAY_BUFFER, this.positionBuffer)
    gl.bufferData(
      gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]),
      gl.STATIC_DRAW,
    )

    const positionLocation = gl.getAttribLocation(this.program, 'aPosition')
    if (positionLocation < 0) throw new Error('WebGL position attribute unavailable')
    gl.enableVertexAttribArray(positionLocation)
    gl.vertexAttribPointer(positionLocation, 2, gl.FLOAT, false, 0, 0)

    this.uniforms.clear()
    for (const name of UNIFORM_NAMES) {
      this.uniforms.set(name, gl.getUniformLocation(this.program, name))
    }
    gl.useProgram(this.program)
    gl.uniform1i(this.uniform('uFrom'), 0)
    gl.uniform1i(this.uniform('uTo'), 1)
    gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true)
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1)
  }

  private uniform(name: UniformName): WebGLUniformLocation | null {
    return this.uniforms.get(name) ?? null
  }

  private bindEvents(): void {
    window.addEventListener('pointermove', this.handlePointerMove, { passive: true })
    window.addEventListener('resize', this.handleWindowResize, { passive: true })
    document.addEventListener('visibilitychange', this.handleVisibilityChange)
    this.canvas.addEventListener('webglcontextlost', this.handleContextLost)
    this.canvas.addEventListener('webglcontextrestored', this.handleContextRestored)
    this.reducedMotionQuery?.addEventListener('change', this.handleMotionPolicyChange)
    this.compactQuery?.addEventListener('change', this.handleMotionPolicyChange)
    this.mobileQuery?.addEventListener('change', this.handleMobileChange)
  }

  private observeHost(): void {
    if (typeof ResizeObserver !== 'undefined') {
      this.resizeObserver = new ResizeObserver(this.resize)
      this.resizeObserver.observe(this.host)
    }
    if (typeof IntersectionObserver !== 'undefined') {
      this.intersectionObserver = new IntersectionObserver(this.handleIntersection, {
        threshold: 0.01,
      })
      this.intersectionObserver.observe(this.host)
    }
  }

  private handlePointerMove = (event: PointerEvent): void => {
    if (this.motionPolicy !== 'full' || !this.stageVisible) return
    this.pointer.targetX = clamp(event.clientX / Math.max(1, window.innerWidth)) * 2 - 1
    this.pointer.targetY = (clamp(event.clientY / Math.max(1, window.innerHeight)) * 2 - 1) * -1
  }

  private handleWindowResize = (): void => {
    if (!this.mobileQuery) this.handleMobileChange()
    this.resize()
  }

  private handleVisibilityChange = (): void => {
    this.documentVisible = !document.hidden
    if (this.documentVisible) this.start()
    else this.stop()
  }

  private handleIntersection = (entries: IntersectionObserverEntry[]): void => {
    const entry = entries[entries.length - 1]
    if (!entry) return
    this.stageVisible = entry.isIntersecting
    if (this.stageVisible) {
      this.draw(performance.now())
      this.start()
    } else {
      this.stop()
    }
  }

  private handleMotionPolicyChange = (): void => {
    this.resize()
    if (this.motionPolicy === 'off') {
      this.fromScene = this.toScene
      this.transition = 0
      this.transitionStartedAt = 0
      this.stop()
      this.draw(performance.now())
    } else {
      this.start()
    }
  }

  private handleMobileChange = (): void => {
    const nextMobile = this.isMobileViewport
    if (nextMobile === this.mobile) return
    this.mobile = nextMobile
    if (this.gl) void this.loadAssets()
  }

  private handleContextLost = (event: Event): void => {
    event.preventDefault()
    this.stop()
    this.ready = false
    this.onReadyChange(false)
    this.program = null
    this.positionBuffer = null
    this.vertexArray = null
    this.textures = []
    this.uniforms.clear()
  }

  private handleContextRestored = (): void => {
    if (this.destroyed || !this.gl) return
    try {
      this.setupWebGl()
      void this.loadAssets()
    } catch {
      this.releaseWebGlResources()
      this.ready = false
      this.onReadyChange(false)
    }
  }

  private async loadAssets(): Promise<void> {
    const gl = this.gl
    if (!gl || this.destroyed) return

    const generation = ++this.loadGeneration
    const sources = JIANGNAN_SCENES.map((scene) =>
      this.mobile ? scene.mobileSrc : scene.desktopSrc,
    )
    this.setReady(false)

    try {
      const images = await Promise.all(sources.map(loadImage))
      if (this.destroyed || generation !== this.loadGeneration || gl.isContextLost()) return

      const nextTextures: WebGLTexture[] = []
      try {
        for (const image of images) nextTextures.push(this.createTexture(image))
      } catch {
        for (const texture of nextTextures) gl.deleteTexture(texture)
        throw new Error('WebGL texture upload failed')
      }

      for (const texture of this.textures) gl.deleteTexture(texture)
      this.textures = nextTextures
      this.images = images
      this.textureSize = [images[0]?.naturalWidth || 1, images[0]?.naturalHeight || 1]
      this.setReady(true)
      this.resize()
      this.draw(performance.now())
      this.start()
    } catch {
      if (!this.destroyed && generation === this.loadGeneration) this.setReady(false)
    }
  }

  private createTexture(image: HTMLImageElement): WebGLTexture {
    const gl = this.gl
    if (!gl) throw new Error('WebGL is unavailable')
    const texture = gl.createTexture()
    if (!texture) throw new Error('WebGL texture allocation failed')

    gl.bindTexture(gl.TEXTURE_2D, texture)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGB, gl.RGB, gl.UNSIGNED_BYTE, image)
    return texture
  }

  private setReady(ready: boolean): void {
    if (this.ready === ready) return
    this.ready = ready
    this.onReadyChange(ready)
  }

  setScene(scene: JiangnanSceneId): void {
    if (scene === this.toScene) return
    if (!this.ready || this.motionPolicy === 'off') {
      this.fromScene = scene
      this.toScene = scene
      this.transition = 0
      this.transitionStartedAt = 0
      this.draw(performance.now())
      return
    }

    this.fromScene = this.toScene
    this.toScene = scene
    this.transition = 0
    this.transitionStartedAt = performance.now()
    this.start()
  }

  setProgress(progress: number): void {
    this.progress = clamp(Number.isFinite(progress) ? progress : 0)
    if (this.motionPolicy === 'off') this.draw(performance.now())
  }

  private resize = (): void => {
    const gl = this.gl
    if (!gl || this.destroyed) return
    const bounds = this.host.getBoundingClientRect()
    const maximumDpr = this.motionPolicy === 'full' ? 1.5 : 1.12
    const dpr = Math.min(window.devicePixelRatio || 1, maximumDpr)
    const width = Math.max(1, Math.round(bounds.width * dpr))
    const height = Math.max(1, Math.round(bounds.height * dpr))
    if (this.canvas.width === width && this.canvas.height === height) return
    this.canvas.width = width
    this.canvas.height = height
    gl.viewport(0, 0, width, height)
    this.draw(performance.now())
  }

  private draw(timestamp: number): void {
    const gl = this.gl
    const program = this.program
    if (
      !gl ||
      !program ||
      !this.ready ||
      this.textures.length !== JIANGNAN_SCENES.length ||
      gl.isContextLost()
    ) {
      return
    }

    if (this.transitionStartedAt > 0) {
      const duration = this.motionPolicy === 'lite' ? 860 : 1120
      this.transition = clamp((timestamp - this.transitionStartedAt) / duration)
      if (this.transition >= 1) {
        this.fromScene = this.toScene
        this.transition = 0
        this.transitionStartedAt = 0
      }
    }

    const policy = this.motionPolicy
    const motion = policy === 'off' ? 0 : policy === 'lite' ? 0.55 : 1
    const fromSpec = JIANGNAN_SCENES[this.fromScene]
    const toSpec = JIANGNAN_SCENES[this.toScene]
    const originSpec = JIANGNAN_SCENES[Math.min(this.fromScene, this.toScene)]
    const fromCamera = this.mobile ? fromSpec.mobileCamera : fromSpec.desktopCamera
    const toCamera = this.mobile ? toSpec.mobileCamera : toSpec.desktopCamera
    const dropCenter = this.mobile ? originSpec.mobileLensOrigin : originSpec.desktopLensOrigin
    const push = this.progress * 0.008 * motion
    const time = (timestamp - this.clockOrigin) / 1000

    this.pointer.x = lerp(this.pointer.x, this.pointer.targetX, 0.035)
    this.pointer.y = lerp(this.pointer.y, this.pointer.targetY, 0.035)

    gl.useProgram(program)
    gl.bindVertexArray(this.vertexArray)
    gl.activeTexture(gl.TEXTURE0)
    gl.bindTexture(gl.TEXTURE_2D, this.textures[this.fromScene] ?? null)
    gl.activeTexture(gl.TEXTURE1)
    gl.bindTexture(gl.TEXTURE_2D, this.textures[this.toScene] ?? null)
    gl.uniform2f(this.uniform('uResolution'), this.canvas.width, this.canvas.height)
    gl.uniform2f(this.uniform('uTextureSize'), this.textureSize[0], this.textureSize[1])
    gl.uniform2f(this.uniform('uPointer'), this.pointer.x, this.pointer.y)
    gl.uniform2f(this.uniform('uFromOffset'), fromCamera.offset[0], fromCamera.offset[1])
    gl.uniform2f(this.uniform('uToOffset'), toCamera.offset[0], toCamera.offset[1])
    gl.uniform2f(this.uniform('uDropCenter'), dropCenter[0], dropCenter[1])
    gl.uniform1f(this.uniform('uFromZoom'), fromCamera.zoom + push)
    gl.uniform1f(this.uniform('uToZoom'), toCamera.zoom + push * 0.62)
    gl.uniform1f(this.uniform('uTransition'), this.transition)
    gl.uniform1f(this.uniform('uTime'), time)
    gl.uniform1f(this.uniform('uRain'), toSpec.rain * (policy === 'lite' ? 0.48 : 1))
    gl.uniform1f(this.uniform('uMotion'), motion)
    gl.uniform1f(this.uniform('uArchiveProgress'), this.progress)
    gl.drawArrays(gl.TRIANGLES, 0, 6)
  }

  private shouldAnimate(): boolean {
    return (
      !this.destroyed &&
      this.ready &&
      this.documentVisible &&
      this.stageVisible &&
      this.motionPolicy !== 'off'
    )
  }

  private queueFrame(): void {
    if (this.frameId || !this.shouldAnimate()) return
    this.frameId = window.requestAnimationFrame(this.tick)
  }

  private tick = (timestamp: number): void => {
    this.frameId = 0
    if (!this.shouldAnimate()) return
    const minimumGap = this.motionPolicy === 'lite' ? 32 : 0
    if (timestamp - this.lastDrawAt >= minimumGap) {
      this.draw(timestamp)
      this.lastDrawAt = timestamp
    }
    this.queueFrame()
  }

  private start(): void {
    if (this.motionPolicy === 'off') {
      this.draw(performance.now())
      return
    }
    this.queueFrame()
  }

  private stop(): void {
    if (!this.frameId) return
    window.cancelAnimationFrame(this.frameId)
    this.frameId = 0
  }

  private releaseWebGlResources(): void {
    const gl = this.gl
    if (!gl || gl.isContextLost()) return
    for (const texture of this.textures) gl.deleteTexture(texture)
    if (this.positionBuffer) gl.deleteBuffer(this.positionBuffer)
    if (this.vertexArray) gl.deleteVertexArray(this.vertexArray)
    if (this.program) gl.deleteProgram(this.program)
    this.textures = []
    this.positionBuffer = null
    this.vertexArray = null
    this.program = null
    this.uniforms.clear()
  }

  destroy(): void {
    if (this.destroyed) return
    this.destroyed = true
    this.loadGeneration += 1
    this.stop()
    this.resizeObserver?.disconnect()
    this.intersectionObserver?.disconnect()
    window.removeEventListener('pointermove', this.handlePointerMove)
    window.removeEventListener('resize', this.handleWindowResize)
    document.removeEventListener('visibilitychange', this.handleVisibilityChange)
    this.canvas.removeEventListener('webglcontextlost', this.handleContextLost)
    this.canvas.removeEventListener('webglcontextrestored', this.handleContextRestored)
    this.reducedMotionQuery?.removeEventListener('change', this.handleMotionPolicyChange)
    this.compactQuery?.removeEventListener('change', this.handleMotionPolicyChange)
    this.mobileQuery?.removeEventListener('change', this.handleMobileChange)
    this.releaseWebGlResources()
    this.gl = null
    this.images = []
    this.onReadyChange(false)
  }
}

/** Connects Vue phase refs to the renderer without exposing scene effects to business logic. */
export function useSceneFx(options: SceneFxOptions) {
  const rendererReady = shallowRef(false)
  let renderer: SceneFxRenderer | null = null

  const stopSceneWatch = watch(options.scene, (scene) => renderer?.setScene(scene))
  const stopProgressWatch = watch(options.progress, (progress) => renderer?.setProgress(progress))

  onMounted(() => {
    if (!options.canvas.value || !options.host.value) return
    renderer = new SceneFxRenderer(
      options.canvas.value,
      options.host.value,
      options.scene.value,
      options.progress.value,
      (ready) => {
        rendererReady.value = ready
      },
    )
  })

  onScopeDispose(() => {
    stopSceneWatch()
    stopProgressWatch()
    renderer?.destroy()
    renderer = null
  })

  return { rendererReady: readonly(rendererReady) }
}
