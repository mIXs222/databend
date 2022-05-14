// Copyright 2021 Datafuse Labs.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::BTreeMap;
use std::sync::Arc;

use common_exception::Result;
use common_meta_types::DatabaseMeta;
use common_planners::CreateDatabasePlan;
use common_planners::PlanNode;
use common_tracing::tracing;
use sqlparser::ast::ObjectName;

use crate::sessions::QueryContext;
use crate::sql::statements::resolve_database;
use crate::sql::statements::AnalyzableStatement;
use crate::sql::statements::AnalyzedResult;

#[derive(Debug, Clone, PartialEq)]
pub struct DfCreateDatabase {
    pub if_not_exists: bool,
    pub name: ObjectName,
    pub engine: String,
    pub engine_options: BTreeMap<String, String>,
    pub options: BTreeMap<String, String>,
}

#[async_trait::async_trait]
impl AnalyzableStatement for DfCreateDatabase {
    #[tracing::instrument(err(Debug), level = "debug", skip(self, ctx), fields(ctx.id = ctx.get_id().as_str()))]
    async fn analyze(&self, ctx: Arc<QueryContext>) -> Result<AnalyzedResult> {
        let tenant = ctx.get_tenant();
        let (catalog, db) = resolve_database(&ctx, &self.name, "CREATE DATABASE")?;
        let if_not_exists = self.if_not_exists;
        let meta = self.database_meta()?;

        Ok(AnalyzedResult::SimpleQuery(Box::new(
            PlanNode::CreateDatabase(CreateDatabasePlan {
                tenant,
                if_not_exists,
                catalog,
                db,
                meta,
            }),
        )))
    }
}

impl DfCreateDatabase {
    fn database_meta(&self) -> Result<DatabaseMeta> {
        Ok(DatabaseMeta {
            engine: self.engine.clone(),
            engine_options: self.engine_options.clone(),
            options: self.options.clone(),
            ..Default::default()
        })
    }
}
