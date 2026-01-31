// This file is required for Expo/React Native SQLite migrations - https://orm.drizzle.team/quick-sqlite/expo

import m0000 from "./0000_marvelous_leech.sql"
import m0001 from "./0001_amused_firebrand.sql"
import m0002 from "./0002_lush_shard.sql"
import m0003 from "./0003_equal_star_brand.sql"
import journal from "./meta/_journal.json"

export default {
  journal,
  migrations: {
    m0000,
    m0001,
    m0002,
    m0003,
  },
}
