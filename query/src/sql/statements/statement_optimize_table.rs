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

use std::sync::Arc;

use common_exception::Result;
use common_planners::Optimization;
use common_planners::OptimizeTablePlan;
use common_planners::PlanNode;
use common_tracing::tracing;
use sqlparser::ast::ObjectName;

use crate::sessions::QueryContext;
use crate::sql::statements::AnalyzableStatement;
use crate::sql::statements::AnalyzedResult;

#[derive(Debug, Clone, PartialEq)]
pub struct DfOptimizeTable {
    pub name: ObjectName,
    pub operation: Optimization,
}

#[async_trait::async_trait]
impl AnalyzableStatement for DfOptimizeTable {
    #[tracing::instrument(err(Debug), level = "debug", skip(self, ctx), fields(ctx.id = ctx.get_id().as_str()))]
    async fn analyze(&self, ctx: Arc<QueryContext>) -> Result<AnalyzedResult> {
        let (catalog, database, table) = super::resolve_table(&ctx, &self.name, "OPTIMIZE TABLE")?;
        let plan_node = OptimizeTablePlan {
            catalog,
            database,
            table,
            operation: self.operation,
        };
        Ok(AnalyzedResult::SimpleQuery(Box::new(
            PlanNode::OptimizeTable(plan_node),
        )))
    }
}
