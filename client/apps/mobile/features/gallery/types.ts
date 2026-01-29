export interface SceneInfo {
  location: string
  time_of_day: string
  atmosphere: string
}

export interface Dialogue {
  speaker: string
  text: string
}

export interface Character {
  name: string
  expression_tags: string[]
  visual_description: string
  position: string
}

export interface AnalysisResult {
  scene_info?: SceneInfo
  dialogue?: Dialogue
  characters?: Character[]
}
