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

use common_arrow::arrow::datatypes::Field;
use common_arrow::arrow::datatypes::Schema;
use common_arrow::arrow::io::parquet::read::column_iter_to_arrays;
use common_arrow::arrow::io::parquet::read::ArrayIter;
use common_arrow::arrow::io::parquet::read::RowGroupDeserializer;
use common_arrow::arrow::io::parquet::write::to_parquet_schema;
use common_arrow::parquet::compression::Compression as ParquetCompression;
use common_arrow::parquet::metadata::ColumnDescriptor;
use common_arrow::parquet::metadata::SchemaDescriptor;
use common_arrow::parquet::read::BasicDecompressor;
use common_arrow::parquet::read::PageMetaData;
use common_arrow::parquet::read::PageReader;
use common_datablocks::DataBlock;
use common_datavalues::DataSchemaRef;
use common_exception::ErrorCode;
use common_exception::Result;
use common_planners::PartInfoPtr;
use common_tracing::tracing;
use common_tracing::tracing::debug_span;
use common_tracing::tracing::Instrument;
use futures::AsyncReadExt;
use futures::StreamExt;
use futures::TryStreamExt;
use opendal::Object;
use opendal::Operator;

use crate::storages::fuse::fuse_part::ColumnMeta;
use crate::storages::fuse::fuse_part::FusePartInfo;
use crate::storages::fuse::meta::Compression;

#[derive(Clone)]
pub struct BlockReader {
    operator: Operator,
    projection: Vec<usize>,
    arrow_schema: Arc<Schema>,
    projected_schema: DataSchemaRef,
    parquet_schema_descriptor: SchemaDescriptor,
}

impl BlockReader {
    pub fn create(
        operator: Operator,
        schema: DataSchemaRef,
        projection: Vec<usize>,
    ) -> Result<Arc<BlockReader>> {
        let projected_schema = DataSchemaRef::new(schema.project(projection.clone()));

        let arrow_schema = schema.to_arrow();
        let parquet_schema_descriptor = to_parquet_schema(&arrow_schema)?;
        Ok(Arc::new(BlockReader {
            operator,
            projection,
            projected_schema,
            parquet_schema_descriptor,
            arrow_schema: Arc::new(arrow_schema),
        }))
    }

    fn to_deserialize(
        meta: &ColumnMeta,
        chunk: Vec<u8>,
        rows: usize,
        column_descriptor: &ColumnDescriptor,
        field: Field,
        compression: &Compression,
    ) -> Result<ArrayIter<'static>> {
        let page_meta_data = PageMetaData {
            column_start: meta.offset,
            num_values: meta.num_values as i64,
            compression: Self::to_parquet_compression(compression),
            descriptor: column_descriptor.descriptor.clone(),
        };
        let pages = PageReader::new_with_page_meta(
            std::io::Cursor::new(chunk),
            page_meta_data,
            Arc::new(|_, _| true),
            vec![],
        );

        let primitive_type = &column_descriptor.descriptor.primitive_type;
        let decompressor = BasicDecompressor::new(pages, vec![]);
        Ok(column_iter_to_arrays(
            vec![decompressor],
            vec![primitive_type],
            field,
            rows,
        )?)
    }

    async fn read_columns(&self, part: PartInfoPtr) -> Result<(usize, Vec<ArrayIter<'static>>)> {
        let part = FusePartInfo::from_part(&part)?;

        let rows = part.nums_rows;
        // TODO: add prefetch column data.
        let num_cols = self.projection.len();
        let mut column_chunk_futs = Vec::with_capacity(num_cols);
        let mut col_idx = Vec::with_capacity(num_cols);
        for index in &self.projection {
            let column_meta = &part.columns_meta[index];
            let column_reader = self.operator.object(&part.location);
            let fut = async move {
                // NOTE: move chunk inside future so that alloc only
                // happen when future is ready to go.
                let column_chunk = column_reader
                    .range_read(column_meta.offset..column_meta.offset + column_meta.length)
                    .await?;
                Ok::<_, ErrorCode>(column_chunk)
            }
            .instrument(debug_span!("read_col_chunk"));
            column_chunk_futs.push(fut);
            col_idx.push(index);
        }

        let chunks = futures::stream::iter(column_chunk_futs)
            .buffered(std::cmp::min(10, num_cols))
            .try_collect::<Vec<_>>()
            .await?;

        let mut columns_array_iter = Vec::with_capacity(num_cols);
        for (i, column_chunk) in chunks.into_iter().enumerate() {
            let idx = *col_idx[i];
            let field = self.arrow_schema.fields[idx].clone();
            let column_descriptor = &self.parquet_schema_descriptor.columns()[idx];
            let column_meta = &part.columns_meta[&idx];
            columns_array_iter.push(Self::to_deserialize(
                column_meta,
                column_chunk,
                rows,
                column_descriptor,
                field,
                &part.compression,
            )?);
        }

        Ok((rows, columns_array_iter))
    }

    pub fn deserialize(&self, part: PartInfoPtr, chunks: Vec<Vec<u8>>) -> Result<DataBlock> {
        if self.projection.len() != chunks.len() {
            return Err(ErrorCode::LogicalError(
                "Columns chunk len must be equals projections len.",
            ));
        }

        let part = FusePartInfo::from_part(&part)?;
        let mut columns_array_iter = Vec::with_capacity(self.projection.len());

        let num_rows = part.nums_rows;
        for (index, column_chunk) in chunks.into_iter().enumerate() {
            let index = self.projection[index];
            let field = self.arrow_schema.fields[index].clone();
            let column_descriptor = &self.parquet_schema_descriptor.columns()[index];
            let column_meta = &part.columns_meta[&index];
            columns_array_iter.push(Self::to_deserialize(
                column_meta,
                column_chunk,
                num_rows,
                column_descriptor,
                field,
                &part.compression,
            )?);
        }

        let mut deserializer = RowGroupDeserializer::new(columns_array_iter, num_rows, None);

        self.try_next_block(&mut deserializer)
    }

    pub async fn read_columns_data(&self, part: PartInfoPtr) -> Result<Vec<Vec<u8>>> {
        let part = FusePartInfo::from_part(&part)?;
        let mut join_handlers = Vec::with_capacity(self.projection.len());

        for index in &self.projection {
            let column_meta = &part.columns_meta[index];

            join_handlers.push(Self::read_column(
                self.operator.object(&part.location),
                column_meta.offset,
                column_meta.length,
            ));
        }

        futures::future::try_join_all(join_handlers).await
    }

    async fn read_column(o: Object, offset: u64, length: u64) -> Result<Vec<u8>> {
        let handler = common_base::base::tokio::spawn(async move {
            let mut chunk = vec![0; length as usize];
            let mut r = o.range_reader(offset..offset + length).await?;
            r.read_exact(&mut chunk).await?;
            Ok(chunk)
        });

        match handler.await {
            Ok(Ok(data)) => Ok(data),
            Ok(Err(cause)) => Err(cause),
            Err(cause) => Err(ErrorCode::TokioError(format!(
                "Cannot join future {:?}",
                cause
            ))),
        }
    }

    #[tracing::instrument(err(Debug), level = "debug", skip_all)]
    pub async fn read(&self, part: PartInfoPtr) -> Result<DataBlock> {
        let (num_rows, columns_array_iter) = self.read_columns(part).await?;
        let mut deserializer = RowGroupDeserializer::new(columns_array_iter, num_rows, None);
        self.try_next_block(&mut deserializer)
    }

    fn try_next_block(&self, deserializer: &mut RowGroupDeserializer) -> Result<DataBlock> {
        match deserializer.next() {
            None => Err(ErrorCode::ParquetError("fail to get a chunk")),
            Some(Err(cause)) => Err(ErrorCode::from(cause)),
            Some(Ok(chunk)) => DataBlock::from_chunk(&self.projected_schema, &chunk),
        }
    }

    fn to_parquet_compression(meta_compression: &Compression) -> ParquetCompression {
        match meta_compression {
            Compression::Lz4 => ParquetCompression::Lz4,
            Compression::Lz4Raw => ParquetCompression::Lz4Raw,
        }
    }
}
