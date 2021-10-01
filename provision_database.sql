
DROP TABLE document_history CASCADE;
DROP TABLE document CASCADE;


CREATE TABLE document (
    id BIGSERIAL PRIMARY KEY,
    name character varying UNIQUE NOT NULL,
    last_modified timestamp with time zone NOT NULL,
    current_revision_id BIGINT NULL
);

CREATE TABLE document_history (
    id BIGSERIAL PRIMARY KEY,
    created_at timestamp with time zone NOT NULL,
    document_id BIGINT NOT NULL,
    modified_by character varying NOT NULL,
    document_data TEXT NOT NULL
);

ALTER TABLE document_history ADD CONSTRAINT fk_document_history_document FOREIGN KEY (document_id) REFERENCES document (id);
CREATE INDEX document_history_document_id ON document_history(document_id);

ALTER TABLE document ADD CONSTRAINT fk_document_document_history FOREIGN KEY (current_revision_id) REFERENCES document_history (id);

