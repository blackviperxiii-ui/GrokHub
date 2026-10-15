Persist worker writes projects.json into the config dir the job was queued for, so a late write cannot land in another test's or session's dir.
