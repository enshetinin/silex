//! Linked views and relations. All writes validate live source and destination pages.
use super::*;

impl Database {
    pub fn add_relation(&mut self, database: &str, name: &str, target: &str) -> Result<String> {
        let tx = self.conn.transaction()?;
        for id in [database, target] {
            ensure!(
                page_kind(&tx, id)? == Some(PageKind::Database),
                "La base de datos no está disponible"
            );
        }
        let id = Uuid::new_v4().to_string();
        tx.execute("INSERT INTO db_properties (id, database_id, name, kind, position)
            VALUES (?1, ?2, ?3, 'relation', (SELECT COALESCE(MAX(position), -1) + 1 FROM db_properties WHERE database_id = ?2))",
            params![id, database, if name.trim().is_empty() { "Relación" } else { name.trim() }])?;
        tx.execute(
            "INSERT INTO db_relations (property_id, target_database_id) VALUES (?1, ?2)",
            params![id, target],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn relation_target(&self, property: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT target_database_id FROM db_relations WHERE property_id = ?1",
                [property],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// A block has its own config; reading an existing card does not write to disk.
    pub fn linked_view_config(&self, block: &str, database: &str) -> Result<ViewConfig> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT config FROM db_linked_views WHERE block_id = ?1 AND database_id = ?2",
                params![block, database],
                |r| r.get(0),
            )
            .optional()?;
        Ok(json.map(|j| ViewConfig::from_json(&j)).unwrap_or_else(|| {
            self.database_views(database)
                .ok()
                .and_then(|v| v.into_iter().next())
                .map(|v| v.config)
                .unwrap_or_default()
        }))
    }

    /// Freeze the initial source settings once. A restored block may still
    /// be only in memory; its retained settings remain usable until autosave.
    pub fn ensure_linked_view_config(&self, block: &str, database: &str) -> Result<ViewConfig> {
        let config = self.linked_view_config(block, database)?;
        self.conn.execute("INSERT OR IGNORE INTO db_linked_views (block_id, database_id, config)
            SELECT b.id, ?2, ?3 FROM blocks b JOIN pages p ON p.id = b.link_page_id
            WHERE b.id = ?1 AND b.kind = 'page_link' AND p.id = ?2 AND p.kind = 'database' AND p.deleted_at IS NULL",
            params![block, database, config.to_json()])?;
        Ok(config)
    }

    pub fn set_linked_view_config(
        &self,
        block: &str,
        database: &str,
        config: &ViewConfig,
    ) -> Result<()> {
        let valid: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM blocks b JOIN pages p ON p.id = b.link_page_id
             WHERE b.id = ?1 AND b.kind = 'page_link' AND p.id = ?2 AND p.kind = 'database' AND p.deleted_at IS NULL)",
            params![block, database], |r| r.get(0))?;
        ensure!(valid, "La vista enlazada ya no está disponible");
        self.conn.execute("INSERT INTO db_linked_views (block_id, database_id, config) VALUES (?1, ?2, ?3)
            ON CONFLICT(block_id) DO UPDATE SET database_id = excluded.database_id, config = excluded.config",
            params![block, database, config.to_json()])?;
        Ok(())
    }
}

pub(super) fn read_relation_options(
    conn: &Connection,
    database: &str,
    options: &mut HashMap<String, Vec<SelectOption>>,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT p.id, r.id, r.title FROM db_properties p
        JOIN db_relations rel ON rel.property_id = p.id
        JOIN pages d ON d.id = rel.target_database_id AND d.deleted_at IS NULL
        JOIN pages r ON r.parent_id = d.id AND r.deleted_at IS NULL AND r.kind = 'page'
        WHERE p.database_id = ?1 AND p.kind = 'relation' ORDER BY r.sort_order, r.id",
    )?;
    let mut rows = stmt.query([database])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(2)?;
        options.entry(row.get(0)?).or_default().push(SelectOption {
            id: row.get(1)?,
            name: if name.trim().is_empty() {
                "Sin título".into()
            } else {
                name
            },
            color: OptionColor::Blue,
        });
    }
    Ok(())
}

pub(super) fn read_relation_values(
    conn: &Connection,
    database: &str,
    values: &mut TableValues,
) -> Result<()> {
    let mut stmt = conn.prepare("SELECT v.page_id, v.property_id, v.target_page_id FROM db_relation_values v
        JOIN db_properties p ON p.id = v.property_id AND p.kind = 'relation' AND p.deleted_at IS NULL
        JOIN db_relations rel ON rel.property_id = p.id
        JOIN pages d ON d.id = rel.target_database_id AND d.deleted_at IS NULL
        JOIN pages t ON t.id = v.target_page_id AND t.parent_id = d.id AND t.deleted_at IS NULL
        JOIN pages r ON r.id = v.page_id AND r.deleted_at IS NULL AND r.parent_id = p.database_id
        WHERE p.database_id = ?1 ORDER BY t.sort_order, t.id")?;
    let mut rows = stmt.query([database])?;
    while let Some(row) = rows.next()? {
        let value = values
            .entry(row.get(0)?)
            .or_default()
            .entry(row.get(1)?)
            .or_insert_with(|| CellValue::Relation(Vec::new()));
        if let CellValue::Relation(ids) = value {
            ids.push(row.get(2)?);
        }
    }
    Ok(())
}

pub(super) fn write_relation(
    tx: &Connection,
    clock: Option<&str>,
    row: &str,
    property: &str,
    value: Option<&CellValue>,
) -> Result<()> {
    let ids = match value {
        None => &[][..],
        Some(CellValue::Relation(ids)) => ids,
        _ => bail!("El valor no es una relación"),
    };
    for id in ids {
        let valid: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM db_relations rel
            JOIN pages d ON d.id = rel.target_database_id AND d.deleted_at IS NULL
            JOIN pages t ON t.parent_id = d.id AND t.deleted_at IS NULL AND t.kind = 'page'
            WHERE rel.property_id = ?1 AND t.id = ?2)",
            params![property, id],
            |r| r.get(0),
        )?;
        ensure!(
            valid,
            "La entrada relacionada no pertenece a la base de destino o está en la papelera"
        );
    }
    tx.execute(
        "DELETE FROM db_relation_values WHERE page_id = ?1 AND property_id = ?2",
        params![row, property],
    )?;
    for id in ids {
        tx.execute("INSERT OR IGNORE INTO db_relation_values (page_id, property_id, target_page_id) VALUES (?1, ?2, ?3)", params![row, property, id])?;
    }
    touch_page(tx, clock, row)
}
