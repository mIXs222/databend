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
use common_meta_types::PrincipalIdentity;
use common_meta_types::UserPrivilegeSet;
use common_planners::PlanNode;
use common_planners::RevokePrivilegePlan;
use common_planners::RevokeRolePlan;
use common_tracing::tracing;

use crate::sessions::QueryContext;
use crate::sql::statements::AnalyzableStatement;
use crate::sql::statements::AnalyzedResult;
use crate::sql::statements::DfGrantObject;

#[derive(Debug, Clone, PartialEq)]
pub struct DfRevokePrivilegeStatement {
    pub principal: PrincipalIdentity,
    pub priv_types: UserPrivilegeSet,
    pub on: DfGrantObject,
}

#[async_trait::async_trait]
impl AnalyzableStatement for DfRevokePrivilegeStatement {
    #[tracing::instrument(err(Debug), level = "debug", skip(self, ctx), fields(ctx.id = ctx.get_id().as_str()))]
    async fn analyze(&self, ctx: Arc<QueryContext>) -> Result<AnalyzedResult> {
        let grant_object = self.on.convert_to_grant_object(ctx);

        // ALL PRIVILEGES have different available privileges set on different grant objects
        let mut priv_types = self.priv_types;
        if priv_types.is_all_privileges() {
            priv_types = grant_object.available_privileges()
        }

        Ok(AnalyzedResult::SimpleQuery(Box::new(
            PlanNode::RevokePrivilege(RevokePrivilegePlan {
                principal: self.principal.clone(),
                on: grant_object,
                priv_types,
            }),
        )))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DfRevokeRoleStatement {
    pub role: String,
    pub principal: PrincipalIdentity,
}

#[async_trait::async_trait]
impl AnalyzableStatement for DfRevokeRoleStatement {
    #[tracing::instrument(err(Debug), level = "debug", skip(self, _ctx), fields(_ctx.id = _ctx.get_id().as_str()))]
    async fn analyze(&self, _ctx: Arc<QueryContext>) -> Result<AnalyzedResult> {
        Ok(AnalyzedResult::SimpleQuery(Box::new(PlanNode::RevokeRole(
            RevokeRolePlan {
                principal: self.principal.clone(),
                role: self.role.clone(),
            },
        ))))
    }
}
