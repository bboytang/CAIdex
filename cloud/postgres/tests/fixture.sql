-- Synthetic identities and sessions only; these are not login or user credentials.
CREATE ROLE caidex_test LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
GRANT caidex_app TO caidex_test;
INSERT INTO caidex.users(user_id,display_name,memory_enabled) VALUES
('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa','A',true),
('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb','B',true),
('cccccccc-cccc-cccc-cccc-cccccccccccc','C',false);
INSERT INTO caidex.auth_sessions(session_id,user_id,access_hash,refresh_hash,client_id,audience,platform,access_expires_at,refresh_expires_at)
SELECT user_id,user_id,public.digest(repeat(letter,64),'sha256'),public.digest(repeat(upper(letter),64),'sha256'),'fixture','caidex-data-v1','test',now()+interval '1 hour',now()+interval '1 day'
FROM (VALUES ('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'::uuid,'a'),('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb'::uuid,'b'),('cccccccc-cccc-cccc-cccc-cccccccccccc'::uuid,'c')) AS f(user_id,letter);
INSERT INTO caidex.projects(project_id,owner_user_id,title) VALUES
('11111111-1111-1111-1111-111111111111','aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa','PA'),
('22222222-2222-2222-2222-222222222222','bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb','PB');
INSERT INTO caidex.conversations(conversation_id,user_id,project_id) VALUES
('aaaaaaaa-0000-0000-0000-000000000001','aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',NULL),
('bbbbbbbb-0000-0000-0000-000000000001','bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb',NULL),
('aaaaaaaa-0000-0000-0000-000000000002','aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa','11111111-1111-1111-1111-111111111111');
INSERT INTO caidex.messages(message_id,conversation_id,user_id,project_id,author_user_id,role,body,state)
SELECT conversation_id,conversation_id,user_id,project_id,user_id,'user','PRIVATE_'||left(user_id::text,1),'complete' FROM caidex.conversations;
INSERT INTO caidex.attachments(attachment_id,message_id,user_id,project_id,object_ref,content_hash,size_bytes,state)
SELECT message_id,message_id,user_id,project_id,'fixture-object-'||message_id::text,public.digest('fixture','sha256'),7,'ready' FROM caidex.messages;
INSERT INTO caidex.memories(memory_id,user_id,project_id,state,privacy)
SELECT conversation_id,user_id,project_id,'active','sync_allowed' FROM caidex.conversations;
INSERT INTO caidex.memory_revisions(revision_id,memory_id,user_id,project_id,author_user_id,content,kind)
SELECT memory_id,memory_id,user_id,project_id,user_id,'PRIVATE_'||left(user_id::text,1),'user_decision' FROM caidex.memories;
UPDATE caidex.memories SET current_revision_id=memory_id;
INSERT INTO caidex.memory_sources(source_id,revision_id,message_id,user_id,project_id,source_kind,source_ref,verification)
SELECT revision_id,revision_id,revision_id,user_id,project_id,'chat','fixture-message','verified' FROM caidex.memory_revisions;
INSERT INTO caidex.embedding_spaces(embedding_space_id,user_id,project_id,provider_ref,endpoint_ref,model_ref,model_revision,preprocessing_version,dimensions,distance)
SELECT memory_id,user_id,project_id,'fixture','loopback-fixture','fixture-model','1',1,3,'l2' FROM caidex.memories;
INSERT INTO caidex.memory_embeddings(embedding_id,revision_id,embedding_space_id,user_id,project_id,dimensions,embedding)
SELECT memory_id,memory_id,memory_id,user_id,project_id,3,CASE WHEN user_id::text LIKE 'b%' THEN '[0,0,0]'::public.vector ELSE '[1,1,1]'::public.vector END FROM caidex.memories;
INSERT INTO caidex.memory_jobs(job_id,user_id,project_id,source_id,base_revision_id,kind,execution_owner_ref,operation_id,cloud_epoch,settings_version)
SELECT memory_id,user_id,project_id,memory_id,memory_id,'embed','fixture-execution-owner',memory_id,1,1 FROM caidex.memories;
INSERT INTO caidex.sync_events(event_id,user_id,project_id,operation_id,entity_id,revision_id,channel,kind,cloud_epoch)
SELECT memory_id,user_id,project_id,memory_id,memory_id,memory_id,'memory','upsert',1 FROM caidex.memories;
INSERT INTO caidex.sync_cursors(cursor_id,user_id,project_id,session_id,session_user_id,channel,cloud_epoch,schema_version,protocol_version)
SELECT memory_id,user_id,project_id,user_id,user_id,'memory',1,1,1 FROM caidex.memories;
INSERT INTO caidex.deletion_tombstones(tombstone_id,user_id,project_id,entity_id,entity_kind,seq,cloud_epoch,retain_until)
SELECT memory_id,user_id,project_id,gen_random_uuid(),'memory',1,1,now()+interval '30 days' FROM caidex.memories;
INSERT INTO caidex.user_identities(user_id,kind,issuer,subject,state) SELECT user_id,'email','fixture-email',display_name||'@example.invalid','pending' FROM caidex.users;
INSERT INTO caidex.auth_credentials(user_id,kind,password_hash,algorithm_version) SELECT user_id,'password','$argon2id$fixture-not-a-password-hash','fixture' FROM caidex.users;
INSERT INTO caidex.security_events(user_id,session_id,kind,outcome) SELECT user_id,user_id,'login','success' FROM caidex.users;
